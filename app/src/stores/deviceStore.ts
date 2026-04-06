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
  } catch {}
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
      } catch {}

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
                }).catch(() => {});
                checkAlerts(device_id, payload.id, payload.value, d.name);
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
              invoke("store_metric", { deviceId: device_id, metricId: "_rssi", value: sys.rssi }).catch(() => {});
              invoke("store_metric", { deviceId: device_id, metricId: "_heap", value: sys.heap_free }).catch(() => {});
              invoke("store_metric", { deviceId: device_id, metricId: "_uptime", value: sys.uptime_s }).catch(() => {});

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
    }).catch(() => {});

    // Load live device list
    get().refreshDevices();
  },
}));
