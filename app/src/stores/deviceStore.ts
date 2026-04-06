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
    }
  } catch (err) {
    console.error("Failed to check alerts:", err);
  }
}

// Check conditional rules and execute actions
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
}

const firedRules = new Map<string, number>();

async function checkRules(deviceId: string, metricId: string, value: number, devices: Device[]) {
  try {
    const rules = await invoke<RuleDef[]>("get_rules");
    for (const rule of rules) {
      if (!rule.enabled) continue;
      if (rule.source_device_id !== deviceId || rule.source_metric_id !== metricId) continue;

      const triggered =
        (rule.condition === "above" && value > rule.threshold) ||
        (rule.condition === "below" && value < rule.threshold);

      if (!triggered) continue;

      // Debounce: 30 seconds between rule fires
      const key = `rule-${rule.id}`;
      const lastFired = firedRules.get(key) || 0;
      if (Date.now() - lastFired < 30000) continue;
      firedRules.set(key, Date.now());

      // Execute action
      const target = devices.find((d) => d.id === rule.target_device_id);
      if (!target || !target.online) continue;

      const val = rule.target_value === "true" ? true
        : rule.target_value === "false" ? false
        : isNaN(Number(rule.target_value)) ? rule.target_value
        : Number(rule.target_value);

      invoke("send_command", {
        deviceId: target.id,
        ip: target.ip,
        port: target.port,
        command: { command: "set", id: rule.target_capability_id, value: val },
      }).catch((err) => console.error("Rule action failed:", err));
    }
  } catch (err) {
    console.error("Failed to check rules:", err);
  }
}

// Fire webhooks for events
async function fireWebhooks(eventType: string, deviceId: string, data: Record<string, unknown>) {
  try {
    const webhooks = await invoke<Array<{ url: string; event_type: string; device_id: string | null; enabled: boolean }>>("get_webhooks");
    for (const wh of webhooks) {
      if (!wh.enabled) continue;
      if (wh.event_type !== eventType) continue;
      if (wh.device_id && wh.device_id !== deviceId) continue;

      try {
        const u = new URL(wh.url);
        if (u.protocol !== "http:" && u.protocol !== "https:") continue;
      } catch {
        console.warn("Webhook has invalid URL:", wh.url);
        continue;
      }

      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 10000);

      fetch(wh.url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ event: eventType, device_id: deviceId, ...data, timestamp: new Date().toISOString() }),
        signal: controller.signal,
      })
        .catch((err) => console.warn(`Webhook to ${wh.url.slice(0, 50)} failed:`, err.message))
        .finally(() => clearTimeout(timeout));
    }
  } catch (err) {
    console.error("Failed to fire webhooks:", err);
  }
}

interface DeviceState {
  devices: Device[];
  initialized: boolean;
  refreshDevices: () => Promise<void>;
  addDeviceByIp: (ip: string, port: number) => Promise<Device>;
  removeDevice: (deviceId: string) => Promise<void>;
  updateCapability: (deviceId: string, capId: string, value: unknown) => void;
  initEventListeners: () => void;
}

export const useDeviceStore = create<DeviceState>((set, get) => ({
  devices: [],
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

  initEventListeners: () => {
    if (get().initialized) return;
    set({ initialized: true });

    // Listen for device discovery events (found/lost/updated)
    listen<{ device: Device; event: string }>("device-discovered", async (e) => {
      const { device, event } = e.payload;

      // Load saved nickname/tags from SQLite
      try {
        const saved = await invoke<{ nickname: string | null; tags: string } | null>(
          "get_saved_device",
          { deviceId: device.id },
        );
        if (saved) {
          device.nickname = saved.nickname || undefined;
          device.tags = saved.tags || undefined;
        }
      } catch (err) {
        console.error("Failed to load saved device:", err);
      }

      // Fire webhooks for device state changes
      if (event === "lost") {
        fireWebhooks("device_offline", device.id, { name: device.name });
      } else if (event === "found") {
        fireWebhooks("device_online", device.id, { name: device.name });
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
                fireWebhooks("sensor_update", device_id, { metric: payload.id, value: payload.value });
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

    // Load saved devices from SQLite (show as offline until rediscovered)
    invoke<Array<{ id: string; name: string; ip: string; port: number; firmware: string; platform: string; nickname: string | null; tags: string }>>(
      "get_saved_devices",
    ).then((saved) => {
      if (saved.length > 0) {
        const offlineDevices: Device[] = saved.map((s) => ({
          id: s.id,
          name: s.name,
          ip: s.ip,
          port: s.port,
          firmware: s.firmware || "",
          platform: s.platform || "",
          capabilities: [],
          system: { rssi: 0, heap_free: 0, uptime_s: 0, chip: "" },
          online: false,
          last_seen: "",
          nickname: s.nickname || undefined,
          tags: s.tags || undefined,
        }));
        set((state) => {
          // Only add devices not already in the list (mDNS may have found them first)
          const existing = new Set(state.devices.map((d) => d.id));
          const newDevices = offlineDevices.filter((d) => !existing.has(d.id));
          return { devices: [...state.devices, ...newDevices] };
        });
      }
    }).catch((err) => console.error("Failed to load saved devices:", err));

    // Load live device list
    get().refreshDevices();
  },
}));
