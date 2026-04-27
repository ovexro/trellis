import { invoke } from "@tauri-apps/api/core";

export interface SketchCapability {
  id: string;
  type: "switch" | "sensor" | "slider" | "color" | "text";
  label: string;
  gpio: string;
  unit: string;
  min: string;
  max: string;
}

type BackendCap =
  | { kind: "switch"; id: string; label: string; gpio: number }
  | { kind: "slider"; id: string; label: string; gpio: number; min: number; max: number }
  | { kind: "color"; id: string; label: string }
  | { kind: "sensor"; id: string; label: string; unit: string; gpio?: number }
  | { kind: "text"; id: string; label: string };

interface SketchSpec {
  device_name: string;
  firmware_version?: string;
  board: "esp32" | "picow";
  capabilities: BackendCap[];
}

function parseGpio(s: string): number {
  const n = parseInt(s, 10);
  return Number.isFinite(n) ? n : 0;
}

function parseFloatOr(s: string, fallback: number): number {
  const n = parseFloat(s);
  return Number.isFinite(n) ? n : fallback;
}

function toBackendCap(c: SketchCapability): BackendCap {
  switch (c.type) {
    case "switch":
      return { kind: "switch", id: c.id, label: c.label, gpio: parseGpio(c.gpio) };
    case "slider":
      return {
        kind: "slider",
        id: c.id,
        label: c.label,
        gpio: parseGpio(c.gpio),
        min: parseFloatOr(c.min, 0),
        max: parseFloatOr(c.max, 100),
      };
    case "color":
      return { kind: "color", id: c.id, label: c.label };
    case "sensor": {
      const cap: BackendCap = { kind: "sensor", id: c.id, label: c.label, unit: c.unit };
      if (c.gpio.trim() !== "") {
        (cap as { gpio?: number }).gpio = parseGpio(c.gpio);
      }
      return cap;
    }
    case "text":
      return { kind: "text", id: c.id, label: c.label };
  }
}

/**
 * Generates the .ino source via the backend's `generate_sketch_command`.
 * The backend owns validation and is the single source of truth — see
 * app/src-tauri/src/sketch_gen.rs.
 */
export async function generateSketch(
  deviceName: string,
  board: "esp32" | "picow",
  capabilities: SketchCapability[],
): Promise<string> {
  const spec: SketchSpec = {
    device_name: deviceName,
    board,
    capabilities: capabilities.map(toBackendCap),
  };
  return await invoke<string>("generate_sketch_command", { spec });
}
