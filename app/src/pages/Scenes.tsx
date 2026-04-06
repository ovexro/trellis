import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Zap, Plus, Trash2, Play } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

interface SceneAction {
  deviceId: string;
  capabilityId: string;
  value: unknown;
}

interface Scene {
  name: string;
  actions: SceneAction[];
}

export default function Scenes() {
  const { devices } = useDeviceStore();
  const [scenes, setScenes] = useState<Scene[]>(() => {
    const saved = localStorage.getItem("trellis-scenes");
    return saved ? JSON.parse(saved) : [];
  });
  const [editing, setEditing] = useState(false);
  const [newScene, setNewScene] = useState<Scene>({ name: "", actions: [] });
  const [running, setRunning] = useState<string | null>(null);

  const onlineDevices = devices.filter((d) => d.online);

  const saveScenes = (updated: Scene[]) => {
    setScenes(updated);
    localStorage.setItem("trellis-scenes", JSON.stringify(updated));
  };

  const addAction = () => {
    if (onlineDevices.length === 0) return;
    const device = onlineDevices[0];
    const cap = device.capabilities[0];
    if (!cap) return;
    setNewScene({
      ...newScene,
      actions: [
        ...newScene.actions,
        { deviceId: device.id, capabilityId: cap.id, value: cap.value },
      ],
    });
  };

  const createScene = () => {
    if (!newScene.name.trim() || newScene.actions.length === 0) return;
    saveScenes([...scenes, newScene]);
    setNewScene({ name: "", actions: [] });
    setEditing(false);
  };

  const deleteScene = (index: number) => {
    if (!confirm(`Delete scene "${scenes[index].name}"? This cannot be undone.`)) return;
    saveScenes(scenes.filter((_, i) => i !== index));
  };

  const runScene = async (scene: Scene) => {
    setRunning(scene.name);
    for (const action of scene.actions) {
      const device = devices.find((d) => d.id === action.deviceId);
      if (!device || !device.online) continue;
      try {
        await invoke("send_command", {
          deviceId: device.id,
          ip: device.ip,
          port: device.port,
          command: { command: "set", id: action.capabilityId, value: action.value },
        });
      } catch (err) {
        console.error(`Scene action failed for ${device.name}:`, err);
      }
    }
    setRunning(null);
  };

  return (
    <div className="max-w-xl">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-zinc-100">Scenes</h1>
          <p className="text-sm text-zinc-500">
            Group actions across multiple devices. One button, everything changes.
          </p>
        </div>
        <button
          onClick={() => setEditing(!editing)}
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
            value={newScene.name}
            onChange={(e) => setNewScene({ ...newScene, name: e.target.value })}
            placeholder="Scene name (e.g., Good Night)"
            className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 mb-3"
            autoFocus
          />

          {newScene.actions.map((action, i) => {
            const device = devices.find((d) => d.id === action.deviceId);
            return (
              <div key={i} className="flex items-center gap-2 mb-2 text-xs">
                <select
                  value={action.deviceId}
                  onChange={(e) => {
                    const actions = [...newScene.actions];
                    actions[i].deviceId = e.target.value;
                    setNewScene({ ...newScene, actions });
                  }}
                  className="flex-1 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-zinc-300"
                >
                  {onlineDevices.map((d) => (
                    <option key={d.id} value={d.id}>{d.name}</option>
                  ))}
                </select>
                <select
                  value={action.capabilityId}
                  onChange={(e) => {
                    const actions = [...newScene.actions];
                    actions[i].capabilityId = e.target.value;
                    setNewScene({ ...newScene, actions });
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
                  value={String(action.value)}
                  onChange={(e) => {
                    const actions = [...newScene.actions];
                    const val = e.target.value;
                    actions[i].value = val === "true" ? true : val === "false" ? false : isNaN(Number(val)) ? val : Number(val);
                    setNewScene({ ...newScene, actions });
                  }}
                  className="w-20 bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-zinc-300"
                  placeholder="value"
                />
                <button
                  onClick={() => setNewScene({ ...newScene, actions: newScene.actions.filter((_, j) => j !== i) })}
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
            <button onClick={() => setEditing(false)} className="px-3 py-1.5 text-zinc-500 text-xs">Cancel</button>
            <button onClick={createScene} className="px-3 py-1.5 bg-trellis-500 text-white rounded-lg text-xs">Create</button>
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
          {scenes.map((scene, i) => (
            <div key={i} className="flex items-center justify-between p-4 bg-zinc-900 border border-zinc-800 rounded-xl">
              <div>
                <h3 className="text-sm font-semibold text-zinc-200">{scene.name}</h3>
                <p className="text-xs text-zinc-500">{scene.actions.length} action{scene.actions.length !== 1 ? "s" : ""}</p>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={() => runScene(scene)}
                  disabled={running === scene.name}
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-trellis-500/10 text-trellis-400 hover:bg-trellis-500/20 rounded-lg text-xs transition-colors"
                >
                  <Play size={12} />
                  {running === scene.name ? "Running..." : "Run"}
                </button>
                <button onClick={() => deleteScene(i)} className="p-1.5 text-zinc-600 hover:text-red-400 transition-colors">
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
