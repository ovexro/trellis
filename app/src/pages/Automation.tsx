import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Clock, GitBranch, Plus, Trash2, ToggleLeft, ToggleRight, Webhook, Loader2, Zap } from "lucide-react";
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
}

interface SceneRef {
  id: number;
  name: string;
  actions: { device_id: string; capability_id: string; value: string }[];
  created_at: string;
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
}

interface WebhookDef {
  id: number;
  event_type: string;
  device_id: string | null;
  url: string;
  label: string;
  enabled: boolean;
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
  const [rSrcDevice, setRSrcDevice] = useState("");
  const [rSrcMetric, setRSrcMetric] = useState("");
  const [rCondition, setRCondition] = useState("above");
  const [rThreshold, setRThreshold] = useState("");
  const [rTgtDevice, setRTgtDevice] = useState("");
  const [rTgtCap, setRTgtCap] = useState("");
  const [rTgtValue, setRTgtValue] = useState("");
  const [rLabel, setRLabel] = useState("");

  // Webhook form
  const [showWebhookForm, setShowWebhookForm] = useState(false);
  const [wEvent, setWEvent] = useState("device_offline");
  const [wDevice, setWDevice] = useState("");
  const [wUrl, setWUrl] = useState("");
  const [wLabel, setWLabel] = useState("");
  const [wError, setWError] = useState("");

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
    if (!rSrcDevice || !rSrcMetric || !rThreshold || !rTgtDevice || !rTgtCap || !rLabel) return;
    setActionLoading("create-rule");
    try {
      await invoke("create_rule", {
        sourceDeviceId: rSrcDevice, sourceMetricId: rSrcMetric,
        condition: rCondition, threshold: parseFloat(rThreshold),
        targetDeviceId: rTgtDevice, targetCapabilityId: rTgtCap,
        targetValue: rTgtValue, label: rLabel,
      });
      setShowRuleForm(false);
      setRLabel("");
      await loadAll();
    } catch (err) {
      console.error("Failed to create rule:", err);
    } finally {
      setActionLoading(null);
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
              {schedules.map((s) => (
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
                    {s.last_run && <p className="text-[11px] text-zinc-600 mt-0.5">Last run: {s.last_run}</p>}
                  </div>
                  <div className="flex items-center gap-1.5">
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
              ))}
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
              <p className="text-[11px] text-zinc-500 uppercase tracking-wider">When...</p>
              <div className="flex gap-2">
                <select value={rSrcDevice} onChange={(e) => setRSrcDevice(e.target.value)}
                  className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                  <option value="">Source device...</option>
                  {onlineDevices.map((d) => <option key={d.id} value={d.id}>{d.nickname || d.name}</option>)}
                </select>
                <select value={rSrcMetric} onChange={(e) => setRSrcMetric(e.target.value)}
                  className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                  <option value="">Sensor...</option>
                  {selectedDevice(rSrcDevice)?.capabilities.filter((c) => c.type === "sensor").map((c) => (
                    <option key={c.id} value={c.id}>{c.label}</option>
                  ))}
                </select>
                <select value={rCondition} onChange={(e) => setRCondition(e.target.value)}
                  className="w-24 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300">
                  <option value="above">above</option>
                  <option value="below">below</option>
                </select>
                <input value={rThreshold} onChange={(e) => setRThreshold(e.target.value)} placeholder="value"
                  className="w-20 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300" type="number" />
              </div>
              <p className="text-[11px] text-zinc-500 uppercase tracking-wider">Then...</p>
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
              {rules.map((r) => (
                <div key={r.id} className={`flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-xl ${!r.enabled ? "opacity-50" : ""}`}>
                  <div>
                    <p className="text-sm font-medium text-zinc-200">{r.label}</p>
                    <p className="text-xs text-zinc-500 mt-0.5">
                      If {selectedDevice(r.source_device_id)?.name || r.source_device_id}.{r.source_metric_id} {r.condition} {r.threshold}
                      {" → "}{selectedDevice(r.target_device_id)?.name || r.target_device_id}.{r.target_capability_id} = {r.target_value}
                    </p>
                  </div>
                  <div className="flex items-center gap-1.5">
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
              ))}
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
                  <option value="device_offline">Device goes offline</option>
                  <option value="device_online">Device comes online</option>
                  <option value="alert_triggered">Alert triggered</option>
                  <option value="sensor_update">Sensor update</option>
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
                <div key={w.id} className={`flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-xl ${!w.enabled ? "opacity-50" : ""}`}>
                  <div>
                    <p className="text-sm font-medium text-zinc-200">{w.label}</p>
                    <p className="text-xs text-zinc-500 mt-0.5">{w.event_type} → <span className="font-mono">{w.url.slice(0, 50)}{w.url.length > 50 ? "..." : ""}</span></p>
                  </div>
                  <div className="flex items-center gap-1.5">
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
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
