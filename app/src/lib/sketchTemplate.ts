import type { SketchCapability } from "@/lib/sketchGenerator";

export const TRELLIS_TEMPLATE_FORMAT = 1;
export const TEMPLATE_FILE_EXT = ".trellis-template.json";

export type SketchBoard = "esp32" | "picow";

export interface SketchTemplateSpec {
  device_name: string;
  board: SketchBoard;
  firmware_version?: string;
  capabilities: SketchCapability[];
}

export interface SketchTemplateFile {
  trellis_template_format: number;
  manifest_version: string;
  spec: SketchTemplateSpec;
}

export interface ImportResult {
  spec: SketchTemplateSpec;
  manifestVersionInFile: string;
  warnings: string[];
}

export function buildTemplateJson(
  spec: SketchTemplateSpec,
  manifestVersion: string,
): string {
  const file: SketchTemplateFile = {
    trellis_template_format: TRELLIS_TEMPLATE_FORMAT,
    manifest_version: manifestVersion,
    spec,
  };
  return JSON.stringify(file, null, 2);
}

export function templateFileName(deviceName: string): string {
  const slug = (deviceName || "template")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40) || "template";
  return slug + TEMPLATE_FILE_EXT;
}

const CAP_FIELDS: Array<keyof SketchCapability> = [
  "id",
  "type",
  "label",
  "gpio",
  "unit",
  "min",
  "max",
];

function coerceCap(
  raw: unknown,
  knownKinds: Set<string>,
  warnings: string[],
): SketchCapability | null {
  if (!raw || typeof raw !== "object") {
    warnings.push("Skipped a capability that was not an object.");
    return null;
  }
  const r = raw as Record<string, unknown>;
  const type = typeof r.type === "string" ? r.type : "";
  if (!knownKinds.has(type)) {
    warnings.push(
      `Skipped capability "${r.id ?? "?"}": kind "${type || "?"}" is not supported by the installed library.`,
    );
    return null;
  }
  const cap: SketchCapability = {
    id: typeof r.id === "string" ? r.id : "",
    type: type as SketchCapability["type"],
    label: typeof r.label === "string" ? r.label : "",
    gpio: typeof r.gpio === "string" ? r.gpio : "",
    unit: typeof r.unit === "string" ? r.unit : "",
    min: typeof r.min === "string" ? r.min : "",
    max: typeof r.max === "string" ? r.max : "",
  };
  for (const k of CAP_FIELDS) {
    if (typeof cap[k] !== "string") {
      // belt-and-braces: every field is forced to string above
      cap[k] = "" as never;
    }
  }
  return cap;
}

export function parseTemplateJson(
  text: string,
  knownCapabilityKinds: string[],
  knownBoards: string[],
): ImportResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    throw new Error("File is not valid JSON.");
  }
  if (!parsed || typeof parsed !== "object") {
    throw new Error("Template file must be a JSON object.");
  }
  const top = parsed as Record<string, unknown>;
  if (top.trellis_template_format !== TRELLIS_TEMPLATE_FORMAT) {
    throw new Error(
      `Unrecognized template format (expected ${TRELLIS_TEMPLATE_FORMAT}, got ${String(top.trellis_template_format)}).`,
    );
  }
  const manifestVersionInFile =
    typeof top.manifest_version === "string" ? top.manifest_version : "";
  const specRaw = top.spec;
  if (!specRaw || typeof specRaw !== "object") {
    throw new Error('Template file is missing a "spec" object.');
  }
  const sr = specRaw as Record<string, unknown>;
  const deviceName = typeof sr.device_name === "string" ? sr.device_name : "";
  if (!deviceName) {
    throw new Error('Template "spec.device_name" is missing or not a string.');
  }
  const warnings: string[] = [];
  const knownKinds = new Set(knownCapabilityKinds);
  const knownBoardSet = new Set(knownBoards);
  let board: SketchBoard;
  if (typeof sr.board === "string" && knownBoardSet.has(sr.board)) {
    board = sr.board as SketchBoard;
  } else {
    if (typeof sr.board === "string" && sr.board) {
      warnings.push(
        `Board "${sr.board}" is not supported by the installed library; falling back to esp32.`,
      );
    } else {
      warnings.push('Template "spec.board" missing; falling back to esp32.');
    }
    board = "esp32";
  }
  const fwVersion =
    typeof sr.firmware_version === "string" ? sr.firmware_version : undefined;
  const capsRaw = Array.isArray(sr.capabilities) ? sr.capabilities : null;
  if (capsRaw === null) {
    throw new Error('Template "spec.capabilities" must be an array.');
  }
  const caps: SketchCapability[] = [];
  for (const c of capsRaw) {
    const coerced = coerceCap(c, knownKinds, warnings);
    if (coerced) caps.push(coerced);
  }
  const spec: SketchTemplateSpec = {
    device_name: deviceName,
    board,
    capabilities: caps,
  };
  if (fwVersion !== undefined) spec.firmware_version = fwVersion;
  return { spec, manifestVersionInFile, warnings };
}
