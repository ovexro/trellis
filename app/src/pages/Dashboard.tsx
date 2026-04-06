import { useEffect, useState, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Radar, Plus, Wifi, Search, FolderOpen, ChevronDown, ChevronRight, Palette, X, Trash2, Pencil } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";
import DeviceCard from "@/components/DeviceCard";
import type { Device, DeviceGroup } from "@/lib/types";

const GROUP_COLORS = [
  '#6366f1', // indigo
  '#8b5cf6', // violet
  '#ec4899', // pink
  '#f43f5e', // rose
  '#f97316', // orange
  '#eab308', // yellow
  '#22c55e', // green
  '#06b6d4', // cyan
];

export default function Dashboard() {
  const { devices, initEventListeners, addDeviceByIp } = useDeviceStore();
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [manualIp, setManualIp] = useState("");
  const [manualPort, setManualPort] = useState("8080");
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState("");
  const [searchQuery, setSearchQuery] = useState("");

  // Group state
  const [groups, setGroups] = useState<DeviceGroup[]>([]);
  const [showGroupManager, setShowGroupManager] = useState(false);
  const [collapsedGroups, setCollapsedGroups] = useState<Set<number | "ungrouped">>(new Set());

  useEffect(() => {
    initEventListeners();
  }, [initEventListeners]);

  const loadGroups = useCallback(async () => {
    try {
      const g = await invoke<DeviceGroup[]>("get_groups");
      setGroups(g);
    } catch (err) {
      console.error("Failed to load groups:", err);
    }
  }, []);

  useEffect(() => {
    loadGroups();
  }, [loadGroups]);

  const onlineCount = devices.filter((d) => d.online).length;

  const handleAdd = async () => {
    if (!manualIp.trim()) return;
    setAdding(true);
    setAddError("");
    try {
      await addDeviceByIp(manualIp.trim(), parseInt(manualPort));
      setShowAddDialog(false);
      setManualIp("");
    } catch (err) {
      setAddError(String(err));
    } finally {
      setAdding(false);
    }
  };

  const toggleCollapsed = (key: number | "ungrouped") => {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleSetDeviceGroup = async (deviceId: string, groupId: number | null) => {
    try {
      await invoke("set_device_group", { deviceId, groupId });
      // Update local device state
      useDeviceStore.setState((state) => ({
        devices: state.devices.map((d) =>
          d.id === deviceId ? { ...d, group_id: groupId ?? undefined } : d,
        ),
      }));
    } catch (err) {
      console.error("Failed to set device group:", err);
    }
  };

  // Filter devices by search
  const filteredDevices = devices.filter((d) => {
    if (!searchQuery) return true;
    const q = searchQuery.toLowerCase();
    return (
      d.name.toLowerCase().includes(q) ||
      (d.nickname || "").toLowerCase().includes(q) ||
      d.id.toLowerCase().includes(q) ||
      d.ip.includes(q) ||
      d.platform.toLowerCase().includes(q) ||
      d.system.chip.toLowerCase().includes(q) ||
      (d.tags || "").toLowerCase().includes(q)
    );
  });

  // Organize devices by group
  const devicesByGroup = new Map<number | "ungrouped", Device[]>();
  for (const group of groups) {
    devicesByGroup.set(group.id, []);
  }
  devicesByGroup.set("ungrouped", []);

  for (const device of filteredDevices) {
    const gid = device.group_id;
    if (gid != null && devicesByGroup.has(gid)) {
      devicesByGroup.get(gid)!.push(device);
    } else {
      devicesByGroup.get("ungrouped")!.push(device);
    }
  }

  if (devices.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-center">
        <div className="border border-dashed border-zinc-800 rounded-2xl p-12 max-w-md">
          <Radar size={56} className="text-zinc-600 mb-5 mx-auto animate-pulse" />
          <h2 className="text-lg font-semibold text-zinc-200 mb-2">
            Scanning for devices...
          </h2>
          <p className="text-sm text-zinc-500 mb-1">
            Devices running the Trellis library will appear automatically.
          </p>
          <p className="text-xs text-zinc-600 mb-6">
            Make sure your devices are on the same network as this computer.
          </p>
          <button
            onClick={() => setShowAddDialog(true)}
            className="flex items-center gap-2 px-5 py-2.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm font-medium transition-colors mx-auto"
          >
            <Plus size={14} />
            Add device by IP
          </button>
        </div>

        {showAddDialog && (
          <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowAddDialog(false)}>
            <div onClick={(e) => e.stopPropagation()}>
              <AddDialog
                ip={manualIp}
                port={manualPort}
                adding={adding}
                error={addError}
                onIpChange={setManualIp}
                onPortChange={setManualPort}
                onAdd={handleAdd}
                onCancel={() => setShowAddDialog(false)}
              />
            </div>
          </div>
        )}
      </div>
    );
  }

  const hasGroups = groups.length > 0;

  return (
    <div>
      <div className="flex items-center justify-between mb-4 gap-3">
        <div className="flex items-center gap-2 text-sm text-zinc-400">
          <Wifi size={14} className={onlineCount > 0 ? "text-trellis-400" : "text-zinc-600"} />
          {onlineCount} of {devices.length} online
        </div>

        <div className="flex items-center gap-2 flex-1 max-w-xs">
          <div className="relative flex-1">
            <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-500" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search devices..."
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg pl-8 pr-3 py-1.5 text-sm text-zinc-300 placeholder-zinc-600 focus:border-trellis-500 focus:outline-none"
            />
          </div>
        </div>

        <div className="flex items-center gap-2">
          <button
            onClick={() => setShowGroupManager(true)}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm bg-zinc-800 hover:bg-zinc-700 text-zinc-300 transition-colors"
          >
            <FolderOpen size={14} />
            Manage Groups
          </button>
          <button
            onClick={() => setShowAddDialog(!showAddDialog)}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm bg-zinc-800 hover:bg-zinc-700 text-zinc-300 transition-colors"
          >
            <Plus size={14} />
            Add by IP
          </button>
        </div>
      </div>

      {showAddDialog && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowAddDialog(false)}>
          <div onClick={(e) => e.stopPropagation()}>
            <AddDialog
              ip={manualIp}
              port={manualPort}
              adding={adding}
              error={addError}
              onIpChange={setManualIp}
              onPortChange={setManualPort}
              onAdd={handleAdd}
              onCancel={() => setShowAddDialog(false)}
            />
          </div>
        </div>
      )}

      {showGroupManager && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowGroupManager(false)}>
          <div onClick={(e) => e.stopPropagation()}>
            <GroupManager
              groups={groups}
              onClose={() => setShowGroupManager(false)}
              onGroupsChanged={loadGroups}
            />
          </div>
        </div>
      )}

      {/* Render devices grouped */}
      {hasGroups ? (
        <div className="space-y-4">
          {groups.map((group) => {
            const groupDevices = devicesByGroup.get(group.id) || [];
            const isCollapsed = collapsedGroups.has(group.id);
            return (
              <GroupSection
                key={group.id}
                label={group.name}
                color={group.color}
                count={groupDevices.length}
                collapsed={isCollapsed}
                onToggle={() => toggleCollapsed(group.id)}
              >
                {groupDevices.map((device) => (
                  <DeviceWithGroupAssign
                    key={device.id}
                    device={device}
                    groups={groups}
                    onSetGroup={handleSetDeviceGroup}
                  />
                ))}
              </GroupSection>
            );
          })}

          {/* Ungrouped section */}
          {(() => {
            const ungrouped = devicesByGroup.get("ungrouped") || [];
            if (ungrouped.length === 0 && groups.length > 0) return null;
            const isCollapsed = collapsedGroups.has("ungrouped");
            return (
              <GroupSection
                label="Ungrouped"
                color="#71717a"
                count={ungrouped.length}
                collapsed={isCollapsed}
                onToggle={() => toggleCollapsed("ungrouped")}
              >
                {ungrouped.map((device) => (
                  <DeviceWithGroupAssign
                    key={device.id}
                    device={device}
                    groups={groups}
                    onSetGroup={handleSetDeviceGroup}
                  />
                ))}
              </GroupSection>
            );
          })()}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
          {filteredDevices.map((device) => (
            <DeviceCard key={device.id} device={device} />
          ))}
        </div>
      )}
    </div>
  );
}

// ── Group Section ─────────────────────────────────────────────────────

function GroupSection({
  label,
  color,
  count,
  collapsed,
  onToggle,
  children,
}: {
  label: string;
  color: string;
  count: number;
  collapsed: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <div>
      <button
        onClick={onToggle}
        className="flex items-center gap-2 w-full text-left mb-3 group"
      >
        {collapsed ? (
          <ChevronRight size={14} className="text-zinc-500" />
        ) : (
          <ChevronDown size={14} className="text-zinc-500" />
        )}
        <span
          className="w-2.5 h-2.5 rounded-full flex-shrink-0"
          style={{ backgroundColor: color }}
        />
        <span className="text-sm font-medium text-zinc-200 group-hover:text-zinc-100 transition-colors">
          {label}
        </span>
        <span className="text-xs text-zinc-500">
          {count} device{count !== 1 ? "s" : ""}
        </span>
        <div className="flex-1 border-b border-zinc-800/60 ml-2" />
      </button>

      {!collapsed && (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-2">
          {children}
        </div>
      )}
    </div>
  );
}

// ── Device Card with Group Assignment Dropdown ────────────────────────

function DeviceWithGroupAssign({
  device,
  groups,
  onSetGroup,
}: {
  device: Device;
  groups: DeviceGroup[];
  onSetGroup: (deviceId: string, groupId: number | null) => void;
}) {
  const [showDropdown, setShowDropdown] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  // Close dropdown on outside click
  useEffect(() => {
    if (!showDropdown) return;
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setShowDropdown(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [showDropdown]);

  const currentGroup = groups.find((g) => g.id === device.group_id);

  return (
    <div className="relative">
      <DeviceCard device={device} />
      {/* Group assignment dot + dropdown trigger */}
      <div className="absolute bottom-3 right-3" ref={dropdownRef}>
        <button
          onClick={(e) => {
            e.stopPropagation();
            setShowDropdown(!showDropdown);
          }}
          className="flex items-center gap-1 px-1.5 py-0.5 rounded text-[11px] text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 transition-colors"
          title="Assign to group"
        >
          <span
            className="w-2 h-2 rounded-full flex-shrink-0"
            style={{ backgroundColor: currentGroup?.color || "#71717a" }}
          />
          <ChevronDown size={10} />
        </button>

        {showDropdown && (
          <div className="absolute bottom-full right-0 mb-1 bg-zinc-900 border border-zinc-700 rounded-lg shadow-xl py-1 min-w-[140px] z-50">
            <button
              onClick={(e) => {
                e.stopPropagation();
                onSetGroup(device.id, null);
                setShowDropdown(false);
              }}
              className={`flex items-center gap-2 w-full px-3 py-1.5 text-xs text-left transition-colors ${
                device.group_id == null
                  ? "text-zinc-200 bg-zinc-800"
                  : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800"
              }`}
            >
              <span className="w-2 h-2 rounded-full bg-zinc-600 flex-shrink-0" />
              Ungrouped
            </button>
            {groups.map((g) => (
              <button
                key={g.id}
                onClick={(e) => {
                  e.stopPropagation();
                  onSetGroup(device.id, g.id);
                  setShowDropdown(false);
                }}
                className={`flex items-center gap-2 w-full px-3 py-1.5 text-xs text-left transition-colors ${
                  device.group_id === g.id
                    ? "text-zinc-200 bg-zinc-800"
                    : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800"
                }`}
              >
                <span
                  className="w-2 h-2 rounded-full flex-shrink-0"
                  style={{ backgroundColor: g.color }}
                />
                {g.name}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ── Group Manager Modal ───────────────────────────────────────────────

function GroupManager({
  groups,
  onClose,
  onGroupsChanged,
}: {
  groups: DeviceGroup[];
  onClose: () => void;
  onGroupsChanged: () => void;
}) {
  const [editingId, setEditingId] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [color, setColor] = useState(GROUP_COLORS[0]);
  const [saving, setSaving] = useState(false);

  const startCreate = () => {
    setEditingId(null);
    setName("");
    setColor(GROUP_COLORS[0]);
  };

  const startEdit = (group: DeviceGroup) => {
    setEditingId(group.id);
    setName(group.name);
    setColor(group.color);
  };

  const handleSave = async () => {
    if (!name.trim()) return;
    setSaving(true);
    try {
      if (editingId != null) {
        await invoke("update_group", { id: editingId, name: name.trim(), color });
      } else {
        await invoke("create_group", { name: name.trim(), color });
      }
      onGroupsChanged();
      setName("");
      setEditingId(null);
    } catch (err) {
      console.error("Failed to save group:", err);
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (id: number) => {
    try {
      await invoke("delete_group", { id });
      onGroupsChanged();
      if (editingId === id) {
        setEditingId(null);
        setName("");
      }
    } catch (err) {
      console.error("Failed to delete group:", err);
    }
  };

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-5 w-[380px] max-h-[80vh] overflow-y-auto">
      <div className="flex items-center justify-between mb-4">
        <h3 className="text-sm font-semibold text-zinc-200 flex items-center gap-2">
          <Palette size={14} className="text-trellis-400" />
          Manage Groups
        </h3>
        <button
          onClick={onClose}
          className="text-zinc-500 hover:text-zinc-300 transition-colors"
        >
          <X size={16} />
        </button>
      </div>

      {/* Existing groups */}
      {groups.length > 0 && (
        <div className="space-y-1.5 mb-4">
          {groups.map((group) => (
            <div
              key={group.id}
              className={`flex items-center gap-2 px-3 py-2 rounded-lg text-sm transition-colors ${
                editingId === group.id
                  ? "bg-zinc-800 border border-zinc-700"
                  : "bg-zinc-800/50 hover:bg-zinc-800"
              }`}
            >
              <span
                className="w-3 h-3 rounded-full flex-shrink-0"
                style={{ backgroundColor: group.color }}
              />
              <span className="text-zinc-300 flex-1 truncate">{group.name}</span>
              <button
                onClick={() => startEdit(group)}
                className="text-zinc-500 hover:text-zinc-300 transition-colors p-0.5"
                title="Edit group"
              >
                <Pencil size={12} />
              </button>
              <button
                onClick={() => handleDelete(group.id)}
                className="text-zinc-500 hover:text-red-400 transition-colors p-0.5"
                title="Delete group"
              >
                <Trash2 size={12} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Create / Edit form */}
      <div className="border-t border-zinc-800 pt-4">
        <p className="text-xs text-zinc-500 mb-2">
          {editingId != null ? "Edit group" : "New group"}
        </p>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSave()}
          placeholder="Group name"
          className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600 focus:border-trellis-500 focus:outline-none mb-3"
          autoFocus
        />

        {/* Color palette */}
        <div className="flex gap-2 mb-4 flex-wrap">
          {GROUP_COLORS.map((c) => (
            <button
              key={c}
              onClick={() => setColor(c)}
              className={`w-6 h-6 rounded-full border-2 transition-all ${
                color === c ? "border-white scale-110" : "border-transparent hover:border-zinc-600"
              }`}
              style={{ backgroundColor: c }}
            />
          ))}
        </div>

        <div className="flex gap-2">
          <button
            onClick={handleSave}
            disabled={saving || !name.trim()}
            className="flex-1 px-3 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
          >
            {saving ? "Saving..." : editingId != null ? "Update" : "Create"}
          </button>
          {editingId != null && (
            <button
              onClick={startCreate}
              className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 rounded-lg text-sm transition-colors"
            >
              Cancel
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ── Add Device Dialog (unchanged) ─────────────────────────────────────

function AddDialog({
  ip, port, adding, error, onIpChange, onPortChange, onAdd, onCancel,
}: {
  ip: string;
  port: string;
  adding: boolean;
  error: string;
  onIpChange: (v: string) => void;
  onPortChange: (v: string) => void;
  onAdd: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="mt-4 mb-4 p-4 bg-zinc-900 border border-zinc-800 rounded-xl max-w-sm w-full">
      <h3 className="text-sm font-semibold text-zinc-300 mb-3">Add Device by IP</h3>
      <div className="flex gap-2 mb-2">
        <input
          type="text"
          value={ip}
          onChange={(e) => onIpChange(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onAdd()}
          placeholder="192.168.1.108"
          className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600"
          autoFocus
        />
        <input
          type="number"
          value={port}
          onChange={(e) => onPortChange(e.target.value)}
          className="w-20 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
        />
      </div>
      {error && (
        <p className="text-xs text-red-400 mb-2">{error}</p>
      )}
      <div className="flex gap-2">
        <button
          onClick={onAdd}
          disabled={adding}
          className="flex-1 px-3 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
        >
          {adding ? "Connecting..." : "Connect"}
        </button>
        <button
          onClick={onCancel}
          className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 rounded-lg text-sm transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
