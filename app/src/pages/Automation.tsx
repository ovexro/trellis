import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Clock, GitBranch, Plus, Trash2, ToggleLeft, ToggleRight, Webhook, Loader2, Zap, Send, Play, ChevronDown, ChevronRight, CheckCircle, XCircle, Copy } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

interface Schedule {
  id: number;
  device_id: string;
  capability_id: string;
  value: string;
  cron: string;
  label: string;
  enabled: boolean;
  last_run: string | null;
  scene_id: number | null;
  next_run: string | null;
}

interface SceneRef {
  id: number;
  name: string;
  actions: { device_id: string; capability_id: string; value: string }[];
  created_at: string;
}

interface RuleCondition {
  device_id: string;
  metric_id: string;
  operator: string;
  threshold: string;
}

interface Rule {
  id: number;
  source_device_id: string;
  source_metric_id: string;
  condition: string;
  threshold: number;
  target_device_id: string;
  target_capability_id: string;
  target_value: string;
  label: string;
  enabled: boolean;
  logic: string;
  conditions: string | null;
  last_triggered: string | null;
  scene_id: number | null;
}

interface WebhookDef {
  id: number;
  event_type: string;
  device_id: string | null;
  url: string;
  label: string;
  enabled: boolean;
  last_delivery?: string | null;
  last_success?: boolean | null;
  success_count?: number;
  failure_count?: number;
}

interface WebhookDelivery {
  id: number;
  webhook_id: number;
  event_type: string;
  status_code: number | null;
  success: boolean;
  error: string | null;
  attempt: number;
  timestamp: string;
}

type Tab = "schedules" | "rules" | "webhooks";

export default function Automation() {
  const { devices } = useDeviceStore();
  const [tab, setTab] = useState<Tab>("schedules");
  const [schedules, setSchedules] = useState<Schedule[]>([]);
  const [rules, setRules] = useState<Rule[]>([]);
  const [webhooks, setWebhooks] = useState<WebhookDef[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<string | null>(null);

  const [scenes, setScenes] = useState<SceneRef[]>([]);

  // Schedule form
  const [showScheduleForm, setShowScheduleForm] = useState(false);
  const [sType, setSType] = useState<"action" | "scene">("action");
  const [sDevice, setSDevice] = useState("");
  const [sCap, setSCap] = useState("");
  const [sValue, setSValue] = useState("");
  const [sCron, setSCron] = useState("0 6 * * *");
  const [sLabel, setSLabel] = useState("");
  const [sSceneId, setSSceneId] = useState<number | null>(null);

  // Rule form
  const [showRuleForm, setShowRuleForm] = useState(false);
  const [rType, setRType] = useState<"action" | "scene">("action");
  const [rConditions, setRConditions] = useState<RuleCondition[]>([
    { device_id: "", metric_id: "", operator: "above", threshold: "" },
  ]);
  const [rLogic, setRLogic] = useState<"and" | "or">("and");
  const [rTgtDevice, setRTgtDevice] = useState("");
  const [rTgtCap, setRTgtCap] = useState("");
  const [rTgtValue, setRTgtValue] = useState("");
  const [rSceneId, setRSceneId] = useState<number | null>(null);
  const [rLabel, setRLabel] = useState("");

  // Webhook form
  const [showWebhookForm, setShowWebhookForm] = useState(false);
  const [wEvent, setWEvent] = useState("device.offline");
  const [wDevice, setWDevice] = useState("");
  const [wUrl, setWUrl] = useState("");
  const [wLabel, setWLabel] = useState("");
  const [wError, setWError] = useState("");

  // Webhook delivery log
  const [expandedWebhook, setExpandedWebhook] = useState<number | null>(null);
  const [deliveries, setDeliveries] = useState<WebhookDelivery[]>([]);
  const [deliveriesLoading, setDeliveriesLoading] = useState(false);

  const onlineDevices = devices.filter((d) => d.online);

  useEffect(() => {
    loadAll();
  }, []);

  const loadAll = async () => {
    setLoading(true);
    try {
      const [s, r, w, sc] = await Promise.all([
        invoke<Schedule[]>("get_schedules"),
        invoke<Rule[]>("get_rules"),
        invoke<WebhookDef[]>("get_webhooks"),
        invoke<SceneRef[]>("get_scenes"),
      ]);
      setSchedules(s);
      setRules(r);
      setWebhooks(w);
      setScenes(sc);
    } catch (err) {
      console.error("Failed to load automation data:", err);
    } finally {
      setLoading(false);
    }
  };

  const createSchedule = async () => {
    if (!sLabel || !sCron) return;
    if (sType === "action" && (!sDevice || !sCap || !sValue)) return;
    if (sType === "scene" && !sSceneId) return;
    setActionLoading("create-schedule");
    try {
      await invoke("create_schedule", {
        deviceId: sType === "action" ? sDevice : "",
        capabilityId: sType === "action" ? sCap : "",
        value: sType === "action" ? sValue : "",
        cron: sCron,
        label: sLabel,
        sceneId: sType === "scene" ? sSceneId : null,
      });
      setShowScheduleForm(false);
      setSLabel("");
      setSValue("");
      setSType("action");
      setSSceneId(null);
      await loadAll();
    } catch (err) {
      console.error("Failed to create schedule:", err);
    } finally {
      setActionLoading(null);
    }
  };

  const createRule = async () => {
    const validConditions = rConditions.filter((c) => c.device_id && c.metric_id && c.threshold);
    if (validConditions.length === 0 || !rLabel) return;
    if (rType === "action" && (!rTgtDevice || !rTgtCap)) return;
    if (rType === "scene" && !rSceneId) return;
    setActionLoading("create-rule");
    try {
      const conditionsJson = JSON.stringify(
        validConditions.map((c) => ({
          device_id: c.device_id,
          metric_id: c.metric_id,
          operator: c.operator,
          threshold: parseFloat(c.threshold),
        })),
      );
      // Use first condition for legacy fields (backward compat)
      const first = validConditions[0];
      await invoke("create_rule", {
        sourceDeviceId: first.device_id, sourceMetricId: first.metric_id,
        condition: first.operator, threshold: parseFloat(first.threshold),
        targetDeviceId: rType === "action" ? rTgtDevice : "",
        targetCapabilityId: rType === "action" ? rTgtCap : "",
        targetValue: rType === "action" ? rTgtValue : "",
        label: rLabel,
        logic: validConditions.length > 1 ? rLogic : "and",
        conditions: validConditions.length > 1 ? conditionsJson : null,
        sceneId: rType === "scene" ? rSceneId : null,
      });
      setShowRuleForm(false);
      setRLabel("");
      setRConditions([{ device_id: "", metric_id: "", operator: "above", threshold: "" }]);
      setRLogic("and");
      setRType("action");
      setRSceneId(null);
      await loadAll();
    } catch (err) {
      console.error("Failed to create rule:", err);
    } finally {
      setActionLoading(null);
    }
  };

  const testWebhook = async (wh: WebhookDef) => {
    setActionLoading(`test-webhook-${wh.id}`);
    try {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 10000);
      const payload = JSON.stringify({
        event: "test", device_id: null,
        message: "Test delivery from Trellis",
        timestamp: new Date().toISOString(),
      });
      const resp = await fetch(wh.url, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: payload,
        signal: controller.signal,
      });
      clearTimeout(timeout);
      await invoke("log_webhook_delivery", {
        webhookId: wh.id, eventType: "test", statusCode: resp.status,
        success: resp.ok, error: null, attempt: 1,
      });
      if (expandedWebhook === wh.id) await loadDeliveries(wh.id);
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      await invoke("log_webhook_delivery", {
        webhookId: wh.id, eventType: "test", statusCode: null,
        success: false, error: errMsg, attempt: 1,
      }).catch(() => {});
      if (expandedWebhook === wh.id) await loadDeliveries(wh.id);
    } finally {
      setActionLoading(null);
    }
  };

  const loadDeliveries = async (webhookId: number) => {
    setDeliveriesLoading(true);
    try {
      const d = await invoke<WebhookDelivery[]>("get_webhook_deliveries", { webhookId, limit: 20 });
      setDeliveries(d);
    } catch {
      setDeliveries([]);
    } finally {
      setDeliveriesLoading(false);
    }
  };

  const toggleDeliveryLog = async (webhookId: number) => {
    if (expandedWebhook === webhookId) {
      setExpandedWebhook(null);
    } else {
      setExpandedWebhook(webhookId);
      await loadDeliveries(webhookId);
    }
  };

  const isValidUrl = (url: string): boolean => {
    try {
      const u = new URL(url);
      return u.protocol === "http:" || u.protocol === "https:";
    } catch {
      return false;
    }
  };

  const createWebhook = async () => {
    if (!wUrl || !wLabel) return;
    if (!isValidUrl(wUrl)) {
      setWError("Enter a valid HTTP or HTTPS URL");
      return;
    }
    setWError("");
    setActionLoading("create-webhook");
    try {
      await invoke("create_webhook", {
        eventType: wEvent, deviceId: wDevice || null, url: wUrl, label: wLabel,
      });
      setShowWebhookForm(false);
      setWLabel("");
      setWUrl("");
      await loadAll();
    } catch (err) {
      console.error("Failed to create webhook:", err);
    } finally {
      setActionLoading(null);
    }
  };

  const handleToggle = async (type: string, id: number, enabled: boolean) => {
    setActionLoading(`toggle-${type}-${id}`);
    try {
      await invoke(`toggle_${type}`, { id, enabled: !enabled });
      await loadAll();
    } catch (err) {
      console.error(`Failed to toggle ${type}:`, err);
    } finally {
      setActionLoading(null);
    }
  };

  const handleDelete = async (type: string, id: number, label: string) => {
    if (!confirm(`Delete "${label}"? This cannot be undone.`)) return;
    setActionLoading(`delete-${type}-${id}`);
    try {
      await invoke(`delete_${type}`, { id });
      await loadAll();
    } catch (err) {
      console.error(`Failed to delete ${type}:`, err);
    } finally {
      setActionLoading(null);
    }
  };

  const handleDuplicate = async (type: string, id: number) => {
    setActionLoading(`duplicate-${type}-${id}`);
    try {
      await invoke(`duplicate_${type}`, { id });
      await loadAll();
    } catch (err) {
      console.error(`Failed to duplicate ${type}:`, err);
      alert(`Duplicate failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setActionLoading(null);
    }
  };

  const runSchedule = async (id: number, label: string) => {
    setActionLoading(`run-schedule-${id}`);
    try {
      await invoke("run_schedule", { id });
      await loadAll();
    } catch (err) {
      console.error(`Failed to run schedule "${label}":`, err);
      alert(`Run failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setActionLoading(null);
    }
  };

  const runRule = async (id: number, label: string) => {
    setActionLoading(`run-rule-${id}`);
    try {
      await invoke("run_rule", { id });
      await loadAll();
    } catch (err) {
      console.error(`Failed to run rule "${label}":`, err);
      alert(`Run failed: ${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setActionLoading(null);
    }
  };

  // "Tomorrow at 06:00" / "Today at 14:30" / "Mon Apr 28 at 09:00" — user's locale, minute precision.
  // Invalid cron or no future occurrence → null (UI shows "—").
  const formatNextRun = (iso: string | null): string | null => {
    if (!iso) return null;
    const dt = new Date(iso);
    if (Number.isNaN(dt.getTime())) return null;
    const now = new Date();
    const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const tomorrow = new Date(midnight.getTime() + 24 * 3600 * 1000);
    const dayAfter = new Date(midnight.getTime() + 48 * 3600 * 1000);
    const time = dt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    if (dt >= midnight && dt < tomorrow) return `Today at ${time}`;
    if (dt >= tomorrow && dt < dayAfter) return `Tomorrow at ${time}`;
    const date = dt.toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" });
    return `${date} at ${time}`;
  };

  const tabs: { id: Tab; icon: typeof Clock; label: string; count: number }[] = [
    { id: "schedules", icon: Clock, label: "Schedules", count: schedules.length },
    { id: "rules", icon: GitBranch, label: "Rules", count: rules.length },
    { id: "webhooks", icon: Webhook, label: "Webhooks", count: webhooks.length },
  ];

  const selectedDevice = (id: string) => devices.find((d) => d.id === id);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 size={24} className="animate-spin text-zinc-500" />
      </div>
    );
  }

  return (
    <div>
      <h1 className="text-xl font-bold text-zinc-100 mb-1">Automation</h1>
      <p className="text-sm text-zinc-500 mb-6">
        Schedules run actions at specific times. Rules trigger actions when sensor values change. Webhooks notify external services.
      </p>

      {/* Tabs */}
      <div className="flex gap-1 mb-6 bg-zinc-900 rounded-xl p-1 border border-zinc-800/50 w-fit">
        {tabs.map((t) => (
          <button
            key={t.id}
            onClick={() => setTab(t.id)}
            className={`flex items-center gap-2 px-4 py-2 rounded-lg text-sm transition-colors ${
              tab === t.id
                ? "bg-trellis-500/10 text-trellis-400 font-medium"
                : "text-zinc-400 hover:text-zinc-200"
            }`}
          >
            <t.icon size={14} />
            {t.label}
            {t.count > 0 && (
              <span className="px-1.5 py-0.5 bg-zinc-800 rounded-full text-[10px]">{t.count}</span>
            )}
          </button>
        ))}
      </div>

      {/* Schedules tab */}
      {tab === "schedules" && (
        <div>
          <div className="flex justify-end mb-4">
            <button onClick={() => setShowScheduleForm(!showScheduleForm)}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium">
              <Plus size={12} /> New Schedule
            </button>
          </div>

          {showScheduleForm && (
            <div className="mb-4 p-4 bg-zinc-900 border border-zinc-800 rounded-xl space-y-3">
              <input value={sLabel} onChange={(e) => setSLabel(e.target.value)} placeholder="Schedule name (e.g., Morning pump)"
                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300" autoFocus />

              {/* Type toggle */}
              <div className="flex gap-1 bg-zinc-800 rounded-lg p-0.5 w-fit">
                <button onClick={() => setSType("action")}
                  className={`px-3 py-1 rounded-md text-xs transition-colors ${sType === "action" ? "bg-zinc-700 text-zinc-200" : "text-zinc-500"}`}>
                  Single Action
                </button>
                <button onClick={() => setSType("scene")} disabled={scenes.length === 0}
                  className={`flex items-center gap-1 px-3 py-1 rounded-md text-xs transition-colors ${sType === "scene" ? "bg-zinc-700 text-zinc-200" : "text-zinc-500"} disabled:opacity-30`}>
                  <Zap size={10} /> Scene
                </button>
              </div>

              {sType === "action" ? (
                <>
                  <div className="grid grid-cols-2 gap-2">
                    <select value={sDevice} onChange={(e) => { setSDevice(e.target.value); setSCap(""); }}
                      className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                      <option value="">Select device...</option>
                      {onlineDevices.map((d) => <option key={d.id} value={d.id}>{d.nickname || d.name}</option>)}
                    </select>
                    <select value={sCap} onChange={(e) => setSCap(e.target.value)}
                      className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                      <option value="">Select capability...</option>
                      {selectedDevice(sDevice)?.capabilities.filter((c) => c.type !== "sensor").map((c) => (
                        <option key={c.id} value={c.id}>{c.label}</option>
                      ))}
                    </select>
                  </div>
                  <div className="grid grid-cols-2 gap-2">
                    <input value={sValue} onChange={(e) => setSValue(e.target.value)} placeholder="Value (true/false/number)"
                      className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300" />
                    <input value={sCron} onChange={(e) => setSCron(e.target.value)} placeholder="Cron (e.g., 0 6 * * *)"
                      className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 font-mono" />
                  </div>
                </>
              ) : (
                <div className="grid grid-cols-2 gap-2">
                  <select value={sSceneId ?? ""} onChange={(e) => setSSceneId(e.target.value ? Number(e.target.value) : null)}
                    className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                    <option value="">Select scene...</option>
                    {scenes.map((sc) => (
                      <option key={sc.id} value={sc.id}>{sc.name} ({sc.actions.length} actions)</option>
                    ))}
                  </select>
                  <input value={sCron} onChange={(e) => setSCron(e.target.value)} placeholder="Cron (e.g., 0 6 * * *)"
                    className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 font-mono" />
                </div>
              )}

              <p className="text-[11px] text-zinc-600">Cron format: minute hour day month weekday. Examples: "0 6 * * *" = 6am daily, "*/5 * * * *" = every 5 min</p>
              <div className="flex gap-2">
                <button onClick={createSchedule} disabled={actionLoading === "create-schedule"}
                  className="px-4 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium disabled:opacity-50">
                  {actionLoading === "create-schedule" ? "Creating..." : "Create"}
                </button>
                <button onClick={() => setShowScheduleForm(false)} className="px-4 py-1.5 text-zinc-400 border border-zinc-700 rounded-lg text-xs">Cancel</button>
              </div>
            </div>
          )}

          {schedules.length === 0 && !showScheduleForm ? (
            <div className="border border-dashed border-zinc-800 rounded-2xl p-12 text-center">
              <Clock size={48} className="mx-auto mb-4 text-zinc-600" />
              <p className="text-sm text-zinc-400 mb-1">No schedules</p>
              <p className="text-xs text-zinc-600">Run device actions at specific times — like turning on a pump every morning.</p>
            </div>
          ) : (
            <div className="space-y-2">
              {schedules.map((s) => {
                const nextRunLabel = s.enabled ? formatNextRun(s.next_run) : null;
                return (
                <div key={s.id} className={`flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-xl ${!s.enabled ? "opacity-50" : ""}`}>
                  <div>
                    <p className="text-sm font-medium text-zinc-200">
                      {s.scene_id != null && <Zap size={12} className="inline mr-1 text-trellis-400" />}
                      {s.label}
                    </p>
                    <p className="text-xs text-zinc-500 font-mono mt-0.5">
                      {s.cron} → {s.scene_id != null
                        ? `Scene: ${scenes.find((sc) => sc.id === s.scene_id)?.name ?? `#${s.scene_id}`}`
                        : `${selectedDevice(s.device_id)?.name || s.device_id}.${s.capability_id} = ${s.value}`}
                    </p>
                    {s.enabled && (
                      <p className="text-[11px] text-zinc-500 mt-0.5">
                        Next: {nextRunLabel ?? <span className="text-zinc-600">—</span>}
                      </p>
                    )}
                    {s.last_run && <p className="text-[11px] text-zinc-600 mt-0.5">Last run: {s.last_run}</p>}
                  </div>
                  <div className="flex items-center gap-1.5">
                    <button onClick={() => runSchedule(s.id, s.label)}
                      disabled={actionLoading === `run-schedule-${s.id}` || !s.enabled}
                      title={s.enabled ? "Run now" : "Enable schedule to run"}
                      className="text-zinc-500 hover:text-trellis-400 disabled:opacity-40 disabled:cursor-not-allowed">
                      {actionLoading === `run-schedule-${s.id}`
                        ? <Loader2 size={14} className="animate-spin" />
                        : <Play size={14} />}
                    </button>
                    <button onClick={() => handleDuplicate("schedule", s.id)}
                      disabled={actionLoading === `duplicate-schedule-${s.id}`}
                      title="Duplicate schedule"
                      className="text-zinc-500 hover:text-trellis-400 disabled:opacity-50">
                      {actionLoading === `duplicate-schedule-${s.id}`
                        ? <Loader2 size={14} className="animate-spin" />
                        : <Copy size={14} />}
                    </button>
                    <button onClick={() => handleToggle("schedule", s.id, s.enabled)}
                      disabled={actionLoading === `toggle-schedule-${s.id}`}
                      className="text-zinc-500 hover:text-zinc-300 disabled:opacity-50">
                      {s.enabled ? <ToggleRight size={18} className="text-trellis-400" /> : <ToggleLeft size={18} />}
                    </button>
                    <button onClick={() => handleDelete("schedule", s.id, s.label)}
                      disabled={actionLoading === `delete-schedule-${s.id}`}
                      className="text-zinc-500 hover:text-red-400 disabled:opacity-50"><Trash2 size={14} /></button>
                  </div>
                </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Rules tab */}
      {tab === "rules" && (
        <div>
          <div className="flex justify-end mb-4">
            <button onClick={() => setShowRuleForm(!showRuleForm)}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium">
              <Plus size={12} /> New Rule
            </button>
          </div>

          {showRuleForm && (
            <div className="mb-4 p-4 bg-zinc-900 border border-zinc-800 rounded-xl space-y-3">
              <input value={rLabel} onChange={(e) => setRLabel(e.target.value)} placeholder="Rule name (e.g., Auto-fan on high temp)"
                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300" autoFocus />
              <div className="flex items-center justify-between">
                <p className="text-[11px] text-zinc-500 uppercase tracking-wider">When...</p>
                {rConditions.length > 1 && (
                  <div className="flex gap-0.5 bg-zinc-800 rounded-md p-0.5">
                    <button onClick={() => setRLogic("and")}
                      className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors ${rLogic === "and" ? "bg-trellis-500/20 text-trellis-400" : "text-zinc-500"}`}>
                      AND
                    </button>
                    <button onClick={() => setRLogic("or")}
                      className={`px-2 py-0.5 rounded text-[10px] font-medium transition-colors ${rLogic === "or" ? "bg-trellis-500/20 text-trellis-400" : "text-zinc-500"}`}>
                      OR
                    </button>
                  </div>
                )}
              </div>
              {rConditions.map((cond, i) => (
                <div key={i} className="space-y-1.5">
                  {i > 0 && (
                    <p className="text-[10px] text-trellis-400 font-medium uppercase text-center">{rLogic}</p>
                  )}
                  <div className="flex gap-2">
                    <select value={cond.device_id} onChange={(e) => {
                      const updated = [...rConditions];
                      updated[i] = { ...updated[i], device_id: e.target.value, metric_id: "" };
                      setRConditions(updated);
                    }}
                      className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                      <option value="">Source device...</option>
                      {onlineDevices.map((d) => <option key={d.id} value={d.id}>{d.nickname || d.name}</option>)}
                    </select>
                    <select value={cond.metric_id} onChange={(e) => {
                      const updated = [...rConditions];
                      updated[i] = { ...updated[i], metric_id: e.target.value };
                      setRConditions(updated);
                    }}
                      className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                      <option value="">Sensor...</option>
                      {selectedDevice(cond.device_id)?.capabilities.filter((c) => c.type === "sensor").map((c) => (
                        <option key={c.id} value={c.id}>{c.label}</option>
                      ))}
                    </select>
                    <select value={cond.operator} onChange={(e) => {
                      const updated = [...rConditions];
                      updated[i] = { ...updated[i], operator: e.target.value };
                      setRConditions(updated);
                    }}
                      className="w-28 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                      <option value="above">above</option>
                      <option value="below">below</option>
                      <option value="equals">equals</option>
                      <option value="not_equals">not equals</option>
                    </select>
                    <input value={cond.threshold} onChange={(e) => {
                      const updated = [...rConditions];
                      updated[i] = { ...updated[i], threshold: e.target.value };
                      setRConditions(updated);
                    }} placeholder="value"
                      className="w-20 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300" type="number" />
                    {rConditions.length > 1 && (
                      <button onClick={() => setRConditions(rConditions.filter((_, j) => j !== i))}
                        className="text-zinc-500 hover:text-red-400 flex-shrink-0 p-1"><Trash2 size={14} /></button>
                    )}
                  </div>
                </div>
              ))}
              <button onClick={() => setRConditions([...rConditions, { device_id: "", metric_id: "", operator: "above", threshold: "" }])}
                className="flex items-center gap-1 text-xs text-trellis-400 hover:text-trellis-300 transition-colors">
                <Plus size={12} /> Add condition
              </button>
              <p className="text-[11px] text-zinc-500 uppercase tracking-wider">Then...</p>
              <div className="flex gap-1 bg-zinc-800 rounded-lg p-0.5 w-fit">
                <button onClick={() => setRType("action")}
                  className={`px-3 py-1 rounded-md text-xs transition-colors ${rType === "action" ? "bg-zinc-700 text-zinc-200" : "text-zinc-500"}`}>
                  Single Action
                </button>
                <button onClick={() => setRType("scene")} disabled={scenes.length === 0}
                  className={`flex items-center gap-1 px-3 py-1 rounded-md text-xs transition-colors ${rType === "scene" ? "bg-zinc-700 text-zinc-200" : "text-zinc-500"} disabled:opacity-30`}>
                  <Zap size={10} /> Scene
                </button>
              </div>
              {rType === "action" ? (
                <div className="flex gap-2">
                  <select value={rTgtDevice} onChange={(e) => setRTgtDevice(e.target.value)}
                    className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                    <option value="">Target device...</option>
                    {onlineDevices.map((d) => <option key={d.id} value={d.id}>{d.nickname || d.name}</option>)}
                  </select>
                  <select value={rTgtCap} onChange={(e) => setRTgtCap(e.target.value)}
                    className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                    <option value="">Capability...</option>
                    {selectedDevice(rTgtDevice)?.capabilities.filter((c) => c.type !== "sensor").map((c) => (
                      <option key={c.id} value={c.id}>{c.label}</option>
                    ))}
                  </select>
                  <input value={rTgtValue} onChange={(e) => setRTgtValue(e.target.value)} placeholder="Set to..."
                    className="w-24 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300" />
                </div>
              ) : (
                <select value={rSceneId ?? ""} onChange={(e) => setRSceneId(e.target.value ? Number(e.target.value) : null)}
                  className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                  <option value="">Select scene...</option>
                  {scenes.map((sc) => (
                    <option key={sc.id} value={sc.id}>{sc.name} ({sc.actions.length} actions)</option>
                  ))}
                </select>
              )}
              <div className="flex gap-2">
                <button onClick={createRule} disabled={actionLoading === "create-rule"}
                  className="px-4 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium disabled:opacity-50">
                  {actionLoading === "create-rule" ? "Creating..." : "Create"}
                </button>
                <button onClick={() => setShowRuleForm(false)} className="px-4 py-1.5 text-zinc-400 border border-zinc-700 rounded-lg text-xs">Cancel</button>
              </div>
            </div>
          )}

          {rules.length === 0 && !showRuleForm ? (
            <div className="border border-dashed border-zinc-800 rounded-2xl p-12 text-center">
              <GitBranch size={48} className="mx-auto mb-4 text-zinc-600" />
              <p className="text-sm text-zinc-400 mb-1">No rules</p>
              <p className="text-xs text-zinc-600">Create if/then rules — like "if temperature above 30, turn on fan."</p>
            </div>
          ) : (
            <div className="space-y-2">
              {rules.map((r) => {
                const conditions: { device_id: string; metric_id: string; operator: string; threshold: number }[] =
                  r.conditions ? (() => { try { return JSON.parse(r.conditions); } catch { return []; } })()
                    : [{ device_id: r.source_device_id, metric_id: r.source_metric_id, operator: r.condition, threshold: r.threshold }];
                const logic = r.logic || "and";
                return (
                <div key={r.id} className={`flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-xl ${!r.enabled ? "opacity-50" : ""}`}>
                  <div>
                    <p className="text-sm font-medium text-zinc-200">
                      {r.scene_id != null && <Zap size={12} className="inline mr-1 text-trellis-400" />}
                      {r.label}
                    </p>
                    <p className="text-xs text-zinc-500 mt-0.5">
                      If {conditions.map((c, i) => (
                        <span key={i}>
                          {i > 0 && <span className="text-trellis-400 mx-1 font-medium">{logic.toUpperCase()}</span>}
                          {(selectedDevice(c.device_id)?.nickname || selectedDevice(c.device_id)?.name || c.device_id)}.{c.metric_id} {c.operator === "not_equals" ? "≠" : c.operator === "equals" ? "=" : c.operator} {c.threshold}
                        </span>
                      ))}
                      {" → "}{r.scene_id != null
                        ? `Scene: ${scenes.find((sc) => sc.id === r.scene_id)?.name ?? `#${r.scene_id}`}`
                        : `${selectedDevice(r.target_device_id)?.nickname || selectedDevice(r.target_device_id)?.name || r.target_device_id}.${r.target_capability_id} = ${r.target_value}`}
                    </p>
                    {r.last_triggered && <p className="text-[11px] text-zinc-600 mt-0.5">Last triggered: {r.last_triggered}</p>}
                  </div>
                  <div className="flex items-center gap-1.5">
                    <button onClick={() => runRule(r.id, r.label)}
                      disabled={actionLoading === `run-rule-${r.id}` || !r.enabled}
                      title={r.enabled ? "Run now (bypass conditions)" : "Enable rule to run"}
                      className="text-zinc-500 hover:text-trellis-400 disabled:opacity-40 disabled:cursor-not-allowed">
                      {actionLoading === `run-rule-${r.id}`
                        ? <Loader2 size={14} className="animate-spin" />
                        : <Play size={14} />}
                    </button>
                    <button onClick={() => handleDuplicate("rule", r.id)}
                      disabled={actionLoading === `duplicate-rule-${r.id}`}
                      title="Duplicate rule"
                      className="text-zinc-500 hover:text-trellis-400 disabled:opacity-50">
                      {actionLoading === `duplicate-rule-${r.id}`
                        ? <Loader2 size={14} className="animate-spin" />
                        : <Copy size={14} />}
                    </button>
                    <button onClick={() => handleToggle("rule", r.id, r.enabled)}
                      disabled={actionLoading === `toggle-rule-${r.id}`}
                      className="text-zinc-500 hover:text-zinc-300 disabled:opacity-50">
                      {r.enabled ? <ToggleRight size={18} className="text-trellis-400" /> : <ToggleLeft size={18} />}
                    </button>
                    <button onClick={() => handleDelete("rule", r.id, r.label)}
                      disabled={actionLoading === `delete-rule-${r.id}`}
                      className="text-zinc-500 hover:text-red-400 disabled:opacity-50"><Trash2 size={14} /></button>
                  </div>
                </div>
                );
              })}
            </div>
          )}
        </div>
      )}

      {/* Webhooks tab */}
      {tab === "webhooks" && (
        <div>
          <div className="flex justify-end mb-4">
            <button onClick={() => setShowWebhookForm(!showWebhookForm)}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium">
              <Plus size={12} /> New Webhook
            </button>
          </div>

          {showWebhookForm && (
            <div className="mb-4 p-4 bg-zinc-900 border border-zinc-800 rounded-xl space-y-3">
              <input value={wLabel} onChange={(e) => setWLabel(e.target.value)} placeholder="Webhook name"
                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300" autoFocus />
              <div className="grid grid-cols-2 gap-2">
                <select value={wEvent} onChange={(e) => setWEvent(e.target.value)}
                  className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                  <option value="device.offline">Device goes offline</option>
                  <option value="device.online">Device comes online</option>
                  <option value="ota_applied">OTA firmware applied</option>
                  <option value="alert.triggered">Alert triggered (planned)</option>
                  <option value="sensor.update">Sensor update (planned)</option>
                </select>
                <select value={wDevice} onChange={(e) => setWDevice(e.target.value)}
                  className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                  <option value="">All devices</option>
                  {devices.map((d) => <option key={d.id} value={d.id}>{d.nickname || d.name}</option>)}
                </select>
              </div>
              <input value={wUrl} onChange={(e) => { setWUrl(e.target.value); setWError(""); }} placeholder="https://hooks.slack.com/... or Discord webhook URL"
                className={`w-full bg-zinc-800 border rounded-lg px-3 py-2 text-sm text-zinc-300 font-mono ${wError ? "border-red-500/50" : "border-zinc-700"}`} />
              {wError && <p className="text-xs text-red-400">{wError}</p>}
              <div className="flex gap-2">
                <button onClick={createWebhook} disabled={actionLoading === "create-webhook"}
                  className="px-4 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium disabled:opacity-50">
                  {actionLoading === "create-webhook" ? "Creating..." : "Create"}
                </button>
                <button onClick={() => { setShowWebhookForm(false); setWError(""); }} className="px-4 py-1.5 text-zinc-400 border border-zinc-700 rounded-lg text-xs">Cancel</button>
              </div>
            </div>
          )}

          {webhooks.length === 0 && !showWebhookForm ? (
            <div className="border border-dashed border-zinc-800 rounded-2xl p-12 text-center">
              <Webhook size={48} className="mx-auto mb-4 text-zinc-600" />
              <p className="text-sm text-zinc-400 mb-1">No webhooks</p>
              <p className="text-xs text-zinc-600">Send HTTP POST to external services when events happen — Discord, Slack, Telegram, or any URL.</p>
            </div>
          ) : (
            <div className="space-y-2">
              {webhooks.map((w) => (
                <div key={w.id} className={`bg-zinc-900 border border-zinc-800 rounded-xl ${!w.enabled ? "opacity-50" : ""}`}>
                  <div className="flex items-center justify-between p-4">
                    <div className="min-w-0 flex-1">
                      <p className="text-sm font-medium text-zinc-200">{w.label}</p>
                      <p className="text-xs text-zinc-500 mt-0.5 truncate">{w.event_type} → <span className="font-mono">{w.url.slice(0, 50)}{w.url.length > 50 ? "..." : ""}</span></p>
                      {((w.success_count || 0) + (w.failure_count || 0)) > 0 && (
                        <p className={`text-[11px] mt-0.5 ${w.last_success === false ? "text-red-400" : "text-zinc-500"}`}>
                          Last delivery: <span className="font-mono">{w.last_delivery || "—"}</span>
                          <span className="mx-1.5 text-zinc-600">·</span>
                          <span className="text-green-500">{w.success_count || 0}✓</span>
                          <span className="mx-1 text-zinc-600">/</span>
                          <span className="text-red-400">{w.failure_count || 0}✗</span>
                        </p>
                      )}
                    </div>
                    <div className="flex items-center gap-1.5 flex-shrink-0 ml-2">
                      <button onClick={() => testWebhook(w)} title="Send test"
                        disabled={actionLoading === `test-webhook-${w.id}`}
                        className="text-zinc-500 hover:text-trellis-400 disabled:opacity-50 p-0.5">
                        {actionLoading === `test-webhook-${w.id}` ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
                      </button>
                      <button onClick={() => toggleDeliveryLog(w.id)} title="Delivery log"
                        className="text-zinc-500 hover:text-zinc-300 p-0.5">
                        {expandedWebhook === w.id ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
                      </button>
                      <button onClick={() => handleDuplicate("webhook", w.id)}
                        disabled={actionLoading === `duplicate-webhook-${w.id}`}
                        title="Duplicate webhook"
                        className="text-zinc-500 hover:text-trellis-400 disabled:opacity-50 p-0.5">
                        {actionLoading === `duplicate-webhook-${w.id}`
                          ? <Loader2 size={14} className="animate-spin" />
                          : <Copy size={14} />}
                      </button>
                      <button onClick={() => handleToggle("webhook", w.id, w.enabled)}
                        disabled={actionLoading === `toggle-webhook-${w.id}`}
                        className="text-zinc-500 hover:text-zinc-300 disabled:opacity-50">
                        {w.enabled ? <ToggleRight size={18} className="text-trellis-400" /> : <ToggleLeft size={18} />}
                      </button>
                      <button onClick={() => handleDelete("webhook", w.id, w.label)}
                        disabled={actionLoading === `delete-webhook-${w.id}`}
                        className="text-zinc-500 hover:text-red-400 disabled:opacity-50"><Trash2 size={14} /></button>
                    </div>
                  </div>
                  {expandedWebhook === w.id && (
                    <div className="border-t border-zinc-800 px-4 py-3">
                      <p className="text-[11px] text-zinc-500 uppercase tracking-wider mb-2">Delivery Log</p>
                      {deliveriesLoading ? (
                        <Loader2 size={14} className="animate-spin text-zinc-500" />
                      ) : deliveries.length === 0 ? (
                        <p className="text-xs text-zinc-600">No deliveries yet</p>
                      ) : (
                        <div className="space-y-1 max-h-48 overflow-y-auto">
                          {deliveries.map((d) => (
                            <div key={d.id} className="flex items-center gap-2 text-xs">
                              {d.success ? <CheckCircle size={12} className="text-green-500 flex-shrink-0" /> : <XCircle size={12} className="text-red-400 flex-shrink-0" />}
                              <span className="text-zinc-400 font-mono w-10 flex-shrink-0">{d.status_code || "ERR"}</span>
                              <span className="text-zinc-500">{d.event_type}</span>
                              {d.attempt > 1 && <span className="text-amber-500 text-[10px]">retry #{d.attempt}</span>}
                              <span className="text-zinc-600 ml-auto flex-shrink-0">{d.timestamp}</span>
                              {d.error && <span className="text-red-400 truncate max-w-[150px]" title={d.error}>{d.error}</span>}
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
