import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Pencil, Check, X } from "lucide-react";

interface DeviceNicknameProps {
  deviceId: string;
  originalName: string;
}

interface SavedDevice {
  nickname: string | null;
  tags: string;
}

export default function DeviceNickname({ deviceId, originalName }: DeviceNicknameProps) {
  const [editing, setEditing] = useState(false);
  const [nickname, setNickname] = useState("");
  const [savedNickname, setSavedNickname] = useState<string | null>(null);
  const [tags, setTags] = useState("");
  const [editingTags, setEditingTags] = useState(false);

  useEffect(() => {
    loadSavedData();
  }, [deviceId]);

  const loadSavedData = async () => {
    try {
      const saved = await invoke<SavedDevice | null>("get_saved_device", { deviceId });
      if (saved) {
        setSavedNickname(saved.nickname);
        setTags(saved.tags || "");
        if (saved.nickname) setNickname(saved.nickname);
      }
    } catch (err) {
      console.error("Failed to load saved device:", err);
    }
  };

  const saveNickname = async () => {
    try {
      await invoke("set_device_nickname", { deviceId, nickname });
      setSavedNickname(nickname || null);
      setEditing(false);
    } catch (err) {
      console.error("Failed to save nickname:", err);
    }
  };

  const saveTags = async () => {
    try {
      await invoke("set_device_tags", { deviceId, tags });
      setEditingTags(false);
    } catch (err) {
      console.error("Failed to save tags:", err);
    }
  };

  const displayName = savedNickname || originalName;

  return (
    <div>
      <div className="flex items-center gap-2">
        {editing ? (
          <div className="flex items-center gap-1">
            <input
              type="text"
              value={nickname}
              onChange={(e) => setNickname(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && saveNickname()}
              placeholder={originalName}
              className="bg-zinc-800 border border-zinc-600 rounded px-2 py-0.5 text-lg font-bold text-zinc-100 w-48"
              autoFocus
            />
            <button onClick={saveNickname} className="p-1 text-trellis-400 hover:text-trellis-300">
              <Check size={16} />
            </button>
            <button onClick={() => setEditing(false)} className="p-1 text-zinc-500 hover:text-zinc-300">
              <X size={16} />
            </button>
          </div>
        ) : (
          <>
            <h1 className="text-2xl font-bold text-zinc-100">{displayName}</h1>
            <button
              onClick={() => { setNickname(savedNickname || ""); setEditing(true); }}
              className="p-1 text-zinc-600 hover:text-zinc-400 transition-colors"
              title="Edit nickname"
            >
              <Pencil size={14} />
            </button>
          </>
        )}
      </div>

      {savedNickname && (
        <p className="text-xs text-zinc-600 mt-0.5">Originally: {originalName}</p>
      )}

      {/* Tags */}
      <div className="flex items-center gap-2 mt-2">
        {editingTags ? (
          <div className="flex items-center gap-1">
            <input
              type="text"
              value={tags}
              onChange={(e) => setTags(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && saveTags()}
              placeholder="kitchen, sensor, outdoor..."
              className="bg-zinc-800 border border-zinc-600 rounded px-2 py-0.5 text-xs text-zinc-300 w-60"
              autoFocus
            />
            <button onClick={saveTags} className="p-0.5 text-trellis-400 hover:text-trellis-300">
              <Check size={12} />
            </button>
            <button onClick={() => setEditingTags(false)} className="p-0.5 text-zinc-500 hover:text-zinc-300">
              <X size={12} />
            </button>
          </div>
        ) : (
          <>
            {tags ? (
              <div className="flex gap-1 flex-wrap">
                {tags.split(",").map((tag) => tag.trim()).filter(Boolean).map((tag) => (
                  <span
                    key={tag}
                    className="px-2 py-0.5 bg-zinc-800 border border-zinc-700 rounded-full text-xs text-zinc-400"
                  >
                    {tag}
                  </span>
                ))}
              </div>
            ) : null}
            <button
              onClick={() => setEditingTags(true)}
              className="text-xs text-zinc-600 hover:text-zinc-400 transition-colors"
            >
              {tags ? "edit tags" : "+ add tags"}
            </button>
          </>
        )}
      </div>
    </div>
  );
}
