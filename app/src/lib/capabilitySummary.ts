// Capability summary line shown on marketplace cards (bundled + saved + community)
// so users can skim a template's contents without clicking. Format:
//   "3 caps · 2 switches · 1 sensor"
// Kinds are emitted in canonical order (switch → sensor → slider → color → text);
// counts of zero are skipped. Pluralization is per-kind ("1 switch" vs "2 switches").

const KIND_ORDER = ["switch", "sensor", "slider", "color", "text"] as const;

const PLURALS: Record<string, string> = {
  switch: "switches",
  sensor: "sensors",
  slider: "sliders",
  color: "colors",
  text: "texts",
};

export function summarizeCapabilities(
  caps: ReadonlyArray<{ type?: string }> | null | undefined,
): string {
  const list = caps ?? [];
  const total = list.length;
  const totalLabel = `${total} cap${total === 1 ? "" : "s"}`;
  if (total === 0) return totalLabel;

  const counts: Record<string, number> = {};
  for (const c of list) {
    const t = c?.type;
    if (typeof t === "string" && t) counts[t] = (counts[t] ?? 0) + 1;
  }

  const parts: string[] = [totalLabel];
  for (const kind of KIND_ORDER) {
    const n = counts[kind];
    if (!n) continue;
    parts.push(`${n} ${n === 1 ? kind : PLURALS[kind]}`);
  }
  // Surface unknown kinds at the tail so a malformed template still produces a
  // sensible summary instead of swallowing them silently.
  for (const kind of Object.keys(counts).sort()) {
    if ((KIND_ORDER as readonly string[]).includes(kind)) continue;
    const n = counts[kind];
    parts.push(`${n} ${kind}`);
  }
  return parts.join(" · ");
}

// Variant for saved templates whose capabilities are stored as a JSON string
// in the database. Returns the summary or a graceful fallback if the payload
// is corrupt.
export function summarizeCapabilitiesJson(json: string | null | undefined): string {
  if (!json) return summarizeCapabilities([]);
  try {
    const parsed = JSON.parse(json);
    if (Array.isArray(parsed)) return summarizeCapabilities(parsed);
  } catch {
    // fall through
  }
  return summarizeCapabilities([]);
}
