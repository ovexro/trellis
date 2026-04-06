import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { Device } from "@/lib/types";

interface DeviceState {
  devices: Device[];
  scanning: boolean;
  scan: () => Promise<void>;
  updateCapability: (deviceId: string, capId: string, value: unknown) => void;
}

export const useDeviceStore = create<DeviceState>((set, get) => ({
  devices: [],
  scanning: false,

  scan: async () => {
    set({ scanning: true });
    try {
      const devices = await invoke<Device[]>("scan_devices");
      set({ devices });
    } catch (err) {
      console.error("Scan failed:", err);
    } finally {
      set({ scanning: false });
    }
  },

  updateCapability: (deviceId, capId, value) => {
    const devices = get().devices.map((d) => {
      if (d.id !== deviceId) return d;
      return {
        ...d,
        capabilities: d.capabilities.map((c) =>
          c.id === capId ? { ...c, value } : c,
        ),
      };
    });
    set({ devices });
  },
}));
