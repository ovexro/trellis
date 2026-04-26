import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import type { Device, WsEvent } from "@/lib/types";

interface AlertRule {
  id: number;
  device_id: string;
  metric_id: string;
  condition: string;
  threshold: number;
  label: string;
  enabled: boolean;
}

// Track which alerts have fired recently to avoid spam
const firedAlerts = new Map<string, number>();

async function checkAlerts(deviceId: string, metricId: string, value: number, deviceName: string) {
  try {
    const alerts = await invoke<AlertRule[]>("get_alerts", { deviceId });
    for (const alert of alerts) {
      if (!alert.enabled || alert.metric_id !== metricId) continue;

      const triggered =
        (alert.condition === "above" && value > alert.threshold) ||
        (alert.condition === "below" && value < alert.threshold);

      if (!triggered) continue;

      // Debounce: don't fire the same alert within 60 seconds
      const key = `${alert.id}`;
      const lastFired = firedAlerts.get(key) || 0;
      if (Date.now() - lastFired < 60000) continue;
      firedAlerts.set(key, Date.now());

      // Send desktop notification
      let permitted = await isPermissionGranted();
      if (!permitted) {
        const result = await requestPermission();
        permitted = result === "granted";
      }
      if (permitted) {
        sendNotification({
          title: `Trellis Alert: ${deviceName}`,
          body: `${alert.label}: ${metricId} is ${value.toFixed(1)} (${alert.condition} ${alert.threshold})`,
        });
      }
      // Backend dispatches alert.triggered webhook + ntfy push (slot 3).
    }
  } catch (err) {
    console.error("Failed to check alerts:", err);
  }
}

// Check conditional rules and execute actions
interface RuleCondition {
  device_id: string;
  metric_id: string;
  operator: string;
  threshold: number;
}

interface RuleDef {
  id: number;
  source_device_id: string;
  source_metric_id: string;
  condition: string;
  threshold: number;
  target_device_id: string;
  target_capability_id: string;
  target_value: string;
  enabled: boolean;
  logic: string;
  conditions: string | null;
}

const firedRules = new Map<string, number>();

function evaluateCondition(cond: RuleCondition, devices: Device[]): boolean {
  const dev = devices.find((d) => d.id === cond.device_id);
  if (!dev || !dev.online) return false;
  const cap = dev.capabilities.find((c) => c.id === cond.metric_id);
  if (!cap || cap.value == null) return false;
  const value = typeof cap.value === "number" ? cap.value
    : typeof cap.value === "boolean" ? (cap.value ? 1 : 0)
    : Number(cap.value);
  if (isNaN(value)) return false;
  switch (cond.operator) {
    case "above": return value > cond.threshold;
    case "below": return value < cond.threshold;
    case "equals": return value === cond.threshold;
    case "not_equals": return value !== cond.threshold;
    default: return false;
  }
}

function getConditions(rule: RuleDef): RuleCondition[] {
  if (rule.conditions) {
    try {
      return JSON.parse(rule.conditions) as RuleCondition[];
    } catch { /* fall through to legacy */ }
  }
  return [{
    device_id: rule.source_device_id,
    metric_id: rule.source_metric_id,
    operator: rule.condition,
    threshold: rule.threshold,
  }];
}

async function checkRules(deviceId: string, metricId: string, _value: number, devices: Device[]) {
  try {
    const rules = await invoke<RuleDef[]>("get_rules");
    for (const rule of rules) {
      if (!rule.enabled) continue;

      const conditions = getConditions(rule);

      // Skip if no condition references the updated device+metric
      if (!conditions.some((c) => c.device_id === deviceId && c.metric_id === metricId)) continue;

      // Evaluate all conditions against current device states
      const logic = rule.logic || "and";
      const results = conditions.map((c) => evaluateCondition(c, devices));
      const triggered = logic === "or"
        ? results.some(Boolean)
        : results.every(Boolean);

      if (!triggered) continue;

      // Debounce: 30 seconds between rule fires
      const key = `rule-${rule.id}`;
      const lastFired = firedRules.get(key) || 0;
      if (Date.now() - lastFired < 30000) continue;
      firedRules.set(key, Date.now());

      // Delegate to the shared Rust fire path so last_triggered is stamped
      // and the same code runs for manual Run now + condition-matched fires.
      invoke("run_rule", { id: rule.id })
        .catch((err) => console.error("Rule action failed:", err));
    }
  } catch (err) {
    console.error("Failed to check rules:", err);
  }
}

interface DeviceState {
  devices: Device[];
  favoriteCapabilities: Set<string>;
  initialized: boolean;
  refreshDevices: () => Promise<void>;
  addDeviceByIp: (ip: string, port: number) => Promise<Device>;
  removeDevice: (deviceId: string) => Promise<void>;
  updateCapability: (deviceId: string, capId: string, value: unknown) => void;
  toggleFavoriteCapability: (deviceId: string, capId: string) => Promise<void>;
  initEventListeners: () => void;
}

export const useDeviceStore = create<DeviceState>((set, get) => ({
  devices: [],
  favoriteCapabilities: new Set<string>(),
  initialized: false,

  refreshDevices: async () => {
    try {
      const devices = await invoke<Device[]>("get_devices");
      set({ devices });
    } catch (err) {
      console.error("Failed to get devices:", err);
    }
  },

  removeDevice: async (deviceId: string) => {
    try {
      await invoke("remove_device", { deviceId });
      set((state) => ({
        devices: state.devices.filter((d) => d.id !== deviceId),
      }));
    } catch (err) {
      console.error("Failed to remove device:", err);
    }
  },

  addDeviceByIp: async (ip: string, port: number) => {
    const device = await invoke<Device>("add_device_by_ip", { ip, port });
    // Device will be added via the event listener, but also update immediately
    set((state) => {
      const exists = state.devices.some((d) => d.id === device.id);
      if (exists) {
        return {
          devices: state.devices.map((d) =>
            d.id === device.id ? device : d,
          ),
        };
      }
      return { devices: [...state.devices, device] };
    });
    return device;
  },

  updateCapability: (deviceId, capId, value) => {
    set((state) => ({
      devices: state.devices.map((d) => {
        if (d.id !== deviceId) return d;
        return {
          ...d,
          capabilities: d.capabilities.map((c) =>
            c.id === capId ? { ...c, value } : c,
          ),
        };
      }),
    }));
  },

  toggleFavoriteCapability: async (deviceId, capId) => {
    const key = `${deviceId}:${capId}`;
    const was = get().favoriteCapabilities.has(key);
    // Optimistic update
    set((state) => {
      const next = new Set(state.favoriteCapabilities);
      if (was) next.delete(key); else next.add(key);
      return { favoriteCapabilities: next };
    });
    try {
      await invoke("toggle_favorite_capability", { deviceId, capabilityId: capId });
    } catch (err) {
      console.error("Failed to toggle favorite capability:", err);
      // Revert
      set((state) => {
        const next = new Set(state.favoriteCapabilities);
        if (was) next.add(key); else next.delete(key);
        return { favoriteCapabilities: next };
      });
    }
  },

  initEventListeners: () => {
    if (get().initialized) return;
    set({ initialized: true });

    // Listen for device discovery events (found/lost/updated)
    listen<{ device: Device; event: string }>("device-discovered", async (e) => {
      const { device, event } = e.payload;

      // Load saved nickname/tags from SQLite
      try {
        const saved = await invoke<{ nickname: string | null; tags: string; group_id: number | null; sort_order: number } | null>(
          "get_saved_device",
          { deviceId: device.id },
        );
        if (saved) {
          device.nickname = saved.nickname || undefined;
          device.tags = saved.tags || undefined;
          device.group_id = saved.group_id ?? undefined;
          device.sort_order = saved.sort_order ?? 0;
        }
      } catch (err) {
        console.error("Failed to load saved device:", err);
      }

      // Backend dispatches device.online / device.offline webhooks (slot 2).
      // Send push notification for device offline (desktop-side ntfy).
      if (event === "lost") {
        invoke<string | null>("get_setting", { key: "ntfy_topic" }).then((topic) => {
          if (topic) {
            invoke("send_ntfy", {
              topic,
              title: "Trellis: Device Offline",
              message: `${device.name} went offline`,
              priority: 3,
            }).catch((err: unknown) => console.error("ntfy offline failed:", err));
          }
        }).catch(() => {});
      }

      set((state) => {
        if (event === "lost") {
          return {
            devices: state.devices.map((d) =>
              d.id === device.id ? { ...d, online: false } : d,
            ),
          };
        }

        // found or updated
        const exists = state.devices.some((d) => d.id === device.id);
        if (exists) {
          return {
            devices: state.devices.map((d) =>
              d.id === device.id ? device : d,
            ),
          };
        }
        return { devices: [...state.devices, device] };
      });
    });

    // Listen for live device events (sensor updates, heartbeats)
    listen<{ device_id: string; event_type: string; payload: WsEvent }>(
      "device-event",
      (e) => {
        const { device_id, event_type, payload } = e.payload;

        set((state) => ({
          devices: state.devices.map((d) => {
            if (d.id !== device_id) return d;

            if (event_type === "update" && payload.id) {
              // Store sensor metrics in DB for charts + check alerts
              const cap = d.capabilities.find((c) => c.id === payload.id);
              if (cap?.type === "sensor" && typeof payload.value === "number") {
                invoke("store_metric", {
                  deviceId: device_id,
                  metricId: payload.id,
                  value: payload.value,
                }).catch((err: unknown) => console.error("Failed to store metric:", err));
                checkAlerts(device_id, payload.id, payload.value, d.name);
                checkRules(device_id, payload.id, payload.value, get().devices);
                // Backend dispatches sensor.update webhook (slot 3).
              }

              return {
                ...d,
                capabilities: d.capabilities.map((c) =>
                  c.id === payload.id ? { ...c, value: payload.value } : c,
                ),
              };
            }

            if (event_type === "heartbeat" && payload.system) {
              // Store system metrics for historical charts
              const sys = payload.system as Device["system"];
              invoke("store_metric", { deviceId: device_id, metricId: "_rssi", value: sys.rssi }).catch((err: unknown) => console.error("Store _rssi failed:", err));
              invoke("store_metric", { deviceId: device_id, metricId: "_heap", value: sys.heap_free }).catch((err: unknown) => console.error("Store _heap failed:", err));
              invoke("store_metric", { deviceId: device_id, metricId: "_uptime", value: sys.uptime_s }).catch((err: unknown) => console.error("Store _uptime failed:", err));
              if (typeof sys.nvs_writes === "number") {
                invoke("store_metric", { deviceId: device_id, metricId: "_nvs_writes", value: sys.nvs_writes }).catch((err: unknown) => console.error("Store _nvs_writes failed:", err));
              }

              return {
                ...d,
                system: sys,
                online: true,
                last_seen: new Date().toISOString(),
              };
            }

            return d;
          }),
        }));
      },
    );

    // Boot: load the backend's device list (which now hydrates saved devices
    // from SQLite as offline placeholders at startup — see Discovery::
    // hydrate_from_db in src-tauri/src/discovery.rs), then enrich each entry
    // with React-only metadata (nickname/tags/group_id) that isn't part of
    // the Rust Device struct. The previous version of this block manufactured
    // ghost offline devices in React, but raced with refreshDevices() and
    // lost cross-subnet devices on every restart. Backend hydration is the
    // single source of truth now; this pass only fills in metadata.
    (async () => {
      await get().refreshDevices();
      try {
        const [saved, favs] = await Promise.all([
          invoke<Array<{ id: string; nickname: string | null; tags: string; group_id: number | null; sort_order: number; notes: string; install_date: string }>>(
            "get_saved_devices",
          ),
          invoke<Array<[string, string]>>("get_favorite_capabilities"),
        ]);
        const favSet = new Set(favs.map(([d, c]) => `${d}:${c}`));
        if (saved.length > 0) {
          const savedById = new Map(saved.map((s) => [s.id, s]));
          set((state) => ({
            favoriteCapabilities: favSet,
            devices: state.devices.map((d) => {
              const s = savedById.get(d.id);
              if (!s) return d;
              return {
                ...d,
                nickname: s.nickname || d.nickname,
                tags: s.tags || d.tags,
                group_id: s.group_id ?? d.group_id,
                sort_order: s.sort_order ?? 0,
                notes: s.notes ?? d.notes,
                install_date: s.install_date ?? d.install_date,
              };
            }),
          }));
        } else {
          set({ favoriteCapabilities: favSet });
        }
      } catch (err) {
        console.error("Failed to load saved device metadata:", err);
      }
    })();
  },
}));
