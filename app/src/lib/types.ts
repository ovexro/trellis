export interface Capability {
  id: string;
  type: "switch" | "slider" | "sensor" | "color" | "text";
  label: string;
  unit?: string;
  min?: number;
  max?: number;
  value: unknown;
}

export interface SystemInfo {
  rssi: number;
  heap_free: number;
  uptime_s: number;
  chip: string;
}

export interface Device {
  id: string;
  name: string;
  ip: string;
  port: number;
  firmware: string;
  platform: string;
  capabilities: Capability[];
  system: SystemInfo;
  online: boolean;
  last_seen: string;
  nickname?: string;
  tags?: string;
  group_id?: number | null;
}

export interface DeviceGroup {
  id: number;
  name: string;
  color: string;
  sort_order: number;
}

export interface SerialPortInfo {
  name: string;
  port_type: string;
  vid?: number;
  pid?: number;
}

export interface WsCommand {
  command: "set" | "ota";
  id?: string;
  value?: unknown;
  url?: string;
}

export interface WsEvent {
  event: "update" | "ota_progress" | "heartbeat";
  id?: string;
  value?: unknown;
  percent?: number;
  system?: SystemInfo;
}
