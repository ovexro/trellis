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
  sort_order?: number;
  favorite?: boolean;
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

export type DiagnosticLevel = "ok" | "warn" | "fail" | "info";

export interface FindingAction {
  label: string;
  action_type: string;
  data: Record<string, unknown>;
}

export interface DiagnosticFinding {
  id: string;
  level: DiagnosticLevel;
  title: string;
  detail: string;
  suggestion?: string;
  action?: FindingAction;
}

export interface DiagnosticReport {
  device_id: string;
  overall: "good" | "attention" | "unhealthy";
  generated_at: string;
  findings: DiagnosticFinding[];
}

export type FleetOverall = "good" | "attention" | "unhealthy";

export interface TopFinding {
  level: DiagnosticLevel;
  title: string;
  detail: string;
}

export interface FleetDeviceEntry {
  device_id: string;
  name: string;
  online: boolean;
  overall: FleetOverall;
  critical: number;
  warnings: number;
  top_finding?: TopFinding | null;
}

export interface FleetReport {
  generated_at: string;
  total: number;
  good: number;
  attention: number;
  unhealthy: number;
  devices: FleetDeviceEntry[];
}
