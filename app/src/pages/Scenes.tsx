import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Zap, Plus, Trash2, Play, Loader2, Pencil, Copy } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

interface SceneAction {
  device_id: string;
  capability_id: string;
  value: string;
}

interface Scene {
  id: number;
  name: string;
  actions: SceneAction[];
  created_at: string;
  last_run?: string | null;
}

export default function Scenes() {
  const { devices } = useDeviceStore();
  const [scenes, setScenes] = useState<Scene[]>([]);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [newName, setNewName] = useState("");
  const [newActions, setNewActions] = useState<SceneAction[]>([]);
  const [running, setRunning] = useState<number | null>(null);
  const [duplicating, setDuplicating] = useState<number | null>(null);

  const onlineDevices = devices.filter((d) => d.online);

  const loadScenes = async () => {
    try {
      const data = await invoke<Scene[]>("get_scenes");
      setScenes(data);
    } catch (err) {
      console.error("Failed to load scenes:", err);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { loadScenes(); }, []);

  const addAction = () => {
    if (onlineDevices.length === 0) return;
    const device = onlineDevices[0];
    const cap = device.capabilities.find((c) => c.type !== "sensor") ?? device.capabilities[0];
    if (!cap) return;
    setNewActions([
      ...newActions,
      {
        device_id: device.id,
        capability_id: cap.id,
        value: String(cap.value ?? ""),
      },
    ]);
  };

  const openCreate = () => {
    setEditingId(null);
    setNewName("");
    setNewActions([]);
    setEditing(true);
  };

  const openEdit = (scene: Scene) => {
    setEditingId(scene.id);
    setNewName(scene.name);
    setNewActions(scene.actions.map((a) => ({ ...a })));
    setEditing(true);
  };

  const cancelEdit = () => {
    setEditing(false);
    setEditingId(null);
    setNewName("");
    setNewActions([]);
  };

  const saveScene = async () => {
    if (!newName.trim() || newActions.length === 0) return;
    try {
      if (editingId !== null) {
        await invoke("update_scene", { id: editingId, name: newName.trim(), actions: newActions });
      } else {
        await invoke("create_scene", { name: newName.trim(), actions: newActions });
      }
      cancelEdit();
      await loadScenes();
    } catch (err) {
      console.error("Failed to save scene:", err);
    }
  };

  const deleteScene = async (scene: Scene) => {
    if (!confirm(`Delete scene "${scene.name}"? This cannot be undone.`)) return;
    try {
      await invoke("delete_scene", { id: scene.id });
      if (editingId === scene.id) cancelEdit();
      await loadScenes();
    } catch (err) {
      console.error("Failed to delete scene:", err);
    }
  };

  const runScene = async (scene: Scene) => {
    setRunning(scene.id);
    try {
      await invoke("run_scene", { id: scene.id });
    } catch (err) {
      console.error("Failed to run scene:", err);
    }
    setRunning(null);
  };

  const duplicateScene = async (scene: Scene) => {
    setDuplicating(scene.id);
    try {
      await invoke("duplicate_scene", { id: scene.id });
      await loadScenes();
    } catch (err) {
      console.error("Failed to duplicate scene:", err);
      alert(`Duplicate failed: ${err instanceof Error ? err.message : String(err)}`);
    }
    setDuplicating(null);
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64">
        <Loader2 className="animate-spin text-zinc-500" size={24} />
      </div>
    );
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-zinc-100">Scenes</h1>
          <p className="text-sm text-zinc-500">
            Group actions across multiple devices. One button, everything changes.
          </p>
        </div>
        <button
          onClick={openCreate}
          className="flex items-center gap-2 px-3 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors"
        >
          <Plus size={14} />
          New Scene
        </button>
      </div>

      {editing && (
        <div className="mb-6 p-4 bg-zinc-900 border border-zinc-800 rounded-xl">
          <input
            type="text"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="Scene name (e.g., Good Night)"
            className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 mb-3"
            autoFocus
          />

          {newActions.map((action, i) => {
            const device = devices.find((d) => d.id === action.device_id);
            const deviceOptions = editingId !== null
              ? devices.filter((d) => d.online || d.id === action.device_id)
              : onlineDevices;
            return (
              <div key={i} className="flex items-center gap-2 mb-2 text-xs">
                <select
                  value={action.device_id}
                  onChange={(e) => {
                    const actions = [...newActions];
                    actions[i] = { ...actions[i], device_id: e.target.value };
                    const dev = devices.find((d) => d.id === e.target.value);
                    const cap = dev?.capabilities.find((c) => c.type !== "sensor") ?? dev?.capabilities[0];
                    if (cap) {
                      actions[i].capability_id = cap.id;
                      actions[i].value = String(cap.value ?? "");
                    }
                    setNewActions(actions);
                  }}
                  className="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-zinc-300"
                >
                  {deviceOptions.map((d) => (
                    <option key={d.id} value={d.id}>{d.name}{!d.online ? " (offline)" : ""}</option>
                  ))}
                </select>
                <select
                  value={action.capability_id}
                  onChange={(e) => {
                    const actions = [...newActions];
                    actions[i] = { ...actions[i], capability_id: e.target.value };
                    setNewActions(actions);
                  }}
                  className="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-zinc-300"
                >
                  {device?.capabilities
                    .filter((c) => c.type !== "sensor")
                    .map((c) => (
                      <option key={c.id} value={c.id}>{c.label}</option>
                    ))}
                </select>
                <input
                  type="text"
                  value={action.value}
                  onChange={(e) => {
                    const actions = [...newActions];
                    actions[i] = { ...actions[i], value: e.target.value };
                    setNewActions(actions);
                  }}
                  className="w-20 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-zinc-300"
                  placeholder="value"
                />
                <button
                  onClick={() => setNewActions(newActions.filter((_, j) => j !== i))}
                  className="p-1 text-zinc-500 hover:text-red-400"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            );
          })}

          <div className="flex gap-2 mt-3">
            <button onClick={addAction} className="px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-xs">
              + Add action
            </button>
            <div className="flex-1" />
            <button onClick={cancelEdit} className="px-3 py-1.5 text-zinc-500 text-xs">Cancel</button>
            <button onClick={saveScene} className="px-3 py-1.5 bg-trellis-500 text-white rounded-lg text-xs">
              {editingId !== null ? "Save" : "Create"}
            </button>
          </div>
        </div>
      )}

      {scenes.length === 0 && !editing ? (
        <div className="border border-dashed border-zinc-800 rounded-2xl p-12 text-center">
          <Zap size={48} className="mx-auto mb-4 text-zinc-600" />
          <h3 className="text-sm font-medium text-zinc-400 mb-1">No scenes yet</h3>
          <p className="text-xs text-zinc-600 max-w-xs mx-auto">
            Scenes let you set multiple devices at once — great for routines like
            "Lights Off" or "Movie Mode."
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {scenes.map((scene) => (
            <div key={scene.id} className="flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-xl">
              <div>
                <h3 className="text-sm font-semibold text-zinc-200">{scene.name}</h3>
                <p className="text-xs text-zinc-500">
                  {scene.actions.length} action{scene.actions.length !== 1 ? "s" : ""}
                  {" — "}
                  {[...new Set(scene.actions.map((a) => {
                    const d = devices.find((dev) => dev.id === a.device_id);
                    return d?.name ?? a.device_id;
                  }))].join(", ")}
                </p>
                {scene.last_run && (
                  <p className="text-[11px] text-zinc-600 mt-0.5">Last run: {scene.last_run}</p>
                )}
              </div>
              <div className="flex gap-2">
                <button
                  onClick={() => openEdit(scene)}
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 hover:text-zinc-200 rounded-lg text-xs transition-colors"
                >
                  <Pencil size={12} />
                  Edit
                </button>
                <button
                  onClick={() => duplicateScene(scene)}
                  disabled={duplicating === scene.id}
                  title="Duplicate scene"
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 hover:text-zinc-200 rounded-lg text-xs transition-colors disabled:opacity-50"
                >
                  {duplicating === scene.id ? <Loader2 size={12} className="animate-spin" /> : <Copy size={12} />}
                  Copy
                </button>
                <button
                  onClick={() => runScene(scene)}
                  disabled={running === scene.id}
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-trellis-500/10 text-trellis-400 hover:bg-trellis-500/20 rounded-lg text-xs transition-colors"
                >
                  {running === scene.id ? <Loader2 size={12} className="animate-spin" /> : <Play size={12} />}
                  {running === scene.id ? "Running..." : "Run"}
                </button>
                <button onClick={() => deleteScene(scene)} className="p-1.5 text-zinc-600 hover:text-red-400 transition-colors">
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
