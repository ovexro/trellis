import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Device, WsEvent } from "@/lib/types";

interface DeviceState {
  devices: Device[];
  initialized: boolean;
  refreshDevices: () => Promise<void>;
  addDeviceByIp: (ip: string, port: number) => Promise<Device>;
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
    listen<{ device: Device; event: string }>("device-discovered", (e) => {
      const { device, event } = e.payload;

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
              // Store sensor metrics in DB for charts
              const cap = d.capabilities.find((c) => c.id === payload.id);
              if (cap?.type === "sensor" && typeof payload.value === "number") {
                invoke("store_metric", {
                  deviceId: device_id,
                  metricId: payload.id,
                  value: payload.value,
                }).catch(() => {}); // Fire and forget
              }

              return {
                ...d,
                capabilities: d.capabilities.map((c) =>
                  c.id === payload.id ? { ...c, value: payload.value } : c,
                ),
              };
            }

            if (event_type === "heartbeat" && payload.system) {
              return {
                ...d,
                system: payload.system as Device["system"],
                online: true,
                last_seen: new Date().toISOString(),
              };
            }

            return d;
          }),
        }));
      },
    );

    // Load initial device list
    get().refreshDevices();
  },
}));
