import { useEffect, useState, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import {
  Map,
  Trash2,
  ImagePlus,
  X,
  GripVertical,
  Thermometer,
  ToggleLeft,
  SlidersHorizontal,
  Palette,
  Type,
  Plus,
  Pencil,
  Layers,
  Grid3x3,
  Minimize2,
} from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";
import type { Capability, Device } from "@/lib/types";

// ─── Types ──────────────────────────────────────────────────────────

interface FloorPlanEntry {
  id: number;
  name: string;
  sort_order: number;
  background: string | null;
}

interface DevicePosition {
  device_id: string;
  floor_id: number;
  x: number;
  y: number;
}

// ─── Constants ──────────────────────────────────────────────────────

const GRID_STEP = 4; // percentage-based grid step (~32px at typical canvas widths)

// ─── Helpers ────────────────────────────────────────────────────────

function snap(v: number, enabled: boolean): number {
  if (!enabled) return v;
  return Math.round(v / GRID_STEP) * GRID_STEP;
}

function clampSnap(v: number, enabled: boolean): number {
  return Math.max(2, Math.min(98, snap(v, enabled)));
}

function capIcon(type: string) {
  switch (type) {
    case "sensor":
      return Thermometer;
    case "switch":
      return ToggleLeft;
    case "slider":
      return SlidersHorizontal;
    case "color":
      return Palette;
    case "text":
      return Type;
    default:
      return Thermometer;
  }
}

function primaryCap(device: Device): Capability | null {
  return (
    device.capabilities.find((c) => c.type === "sensor") ||
    device.capabilities.find((c) => c.type === "switch") ||
    device.capabilities[0] ||
    null
  );
}

function capSummary(cap: Capability): string {
  switch (cap.type) {
    case "sensor": {
      const v = cap.value as number;
      return `${typeof v === "number" ? v.toFixed(1) : v}${cap.unit ? ` ${cap.unit}` : ""}`;
    }
    case "switch":
      return cap.value ? "ON" : "OFF";
    case "slider":
      return `${cap.value}${cap.unit ? ` ${cap.unit}` : ""}`;
    case "color":
      return String(cap.value || "#000");
    case "text":
      return String(cap.value || "\u2014");
    default:
      return "";
  }
}

// ─── Draggable device card in the sidebar ──────────────────────────

function UnplacedCard({
  device,
  onDragStart,
}: {
  device: Device;
  onDragStart: (e: React.DragEvent, deviceId: string) => void;
}) {
  const cap = primaryCap(device);
  const Icon = cap ? capIcon(cap.type) : Thermometer;

  return (
    <div
      draggable
      onDragStart={(e) => onDragStart(e, device.id)}
      className="flex items-center gap-2 px-2.5 py-2 bg-zinc-800/60 rounded-lg cursor-grab active:cursor-grabbing hover:bg-zinc-700/60 transition-colors group"
    >
      <GripVertical size={14} className="text-zinc-600 group-hover:text-zinc-400 flex-shrink-0" />
      <span
        className={`w-2 h-2 rounded-full flex-shrink-0 ${
          device.online ? "bg-emerald-400" : "bg-zinc-600"
        }`}
      />
      <span className="text-sm text-zinc-300 truncate flex-1">
        {device.nickname || device.name}
      </span>
      {cap && (
        <span className="text-[11px] text-zinc-500 flex-shrink-0 flex items-center gap-1">
          <Icon size={10} />
          {capSummary(cap)}
        </span>
      )}
    </div>
  );
}

// ─── Placed device node on the canvas ──────────────────────────────

function PlacedNode({
  device,
  pos,
  selected,
  compact,
  onSelect,
  onDragStart,
}: {
  device: Device;
  pos: DevicePosition;
  selected: boolean;
  compact: boolean;
  onSelect: () => void;
  onDragStart: (e: React.MouseEvent) => void;
}) {
  const cap = primaryCap(device);
  const Icon = cap ? capIcon(cap.type) : Thermometer;

  return (
    <div
      onMouseDown={(e) => {
        e.preventDefault();
        onSelect();
        onDragStart(e);
      }}
      style={{
        position: "absolute",
        left: `${pos.x}%`,
        top: `${pos.y}%`,
        transform: "translate(-50%, -50%)",
      }}
      className={`select-none cursor-move group ${
        selected ? "z-20" : "z-10"
      }`}
    >
      <div
        className={`flex flex-col items-center gap-0.5 rounded-xl border transition-all duration-150 ${
          compact ? "px-2 py-1" : "px-3 py-2"
        } ${
          selected
            ? "bg-zinc-800 border-trellis-500 shadow-lg shadow-trellis-500/10"
            : "bg-zinc-800/90 border-zinc-700/50 hover:border-zinc-600"
        }`}
      >
        <div className="flex items-center gap-1.5">
          <span
            className={`w-2 h-2 rounded-full ${
              device.online ? "bg-emerald-400" : "bg-zinc-600"
            }`}
          />
          {cap && <Icon size={compact ? 11 : 13} className="text-trellis-400" />}
          {cap && (
            <span
              className={`font-mono font-bold ${compact ? "text-xs" : "text-sm"} ${
                device.online ? "text-zinc-100" : "text-zinc-500"
              }`}
            >
              {capSummary(cap)}
            </span>
          )}
        </div>
        {!compact && (
          <span className="text-[11px] text-zinc-400 max-w-[120px] truncate text-center">
            {device.nickname || device.name}
          </span>
        )}
      </div>
    </div>
  );
}

// ─── Inline control popup ──────────────────────────────────────────

function InlineControl({
  device,
  onClose,
  onNavigate,
}: {
  device: Device;
  onClose: () => void;
  onNavigate: () => void;
}) {
  const { updateCapability } = useDeviceStore();

  const handleChange = async (capId: string, value: unknown) => {
    updateCapability(device.id, capId, value);
    try {
      await invoke("send_command", {
        deviceId: device.id,
        ip: device.ip,
        port: device.port,
        command: { command: "set", id: capId, value },
      });
    } catch (err) {
      console.error("Failed to send command:", err);
    }
  };

  const controllable = device.capabilities.filter(
    (c) => c.type === "switch" || c.type === "slider"
  );

  return (
    <div className="bg-zinc-900 border border-zinc-700 rounded-xl shadow-xl p-3 min-w-[200px] max-w-[280px]">
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm font-medium text-zinc-200">
          {device.nickname || device.name}
        </span>
        <button
          onClick={onClose}
          className="text-zinc-500 hover:text-zinc-300 transition-colors"
        >
          <X size={14} />
        </button>
      </div>

      <div className="space-y-2 mb-2">
        {device.capabilities.map((cap) => {
          if (cap.type === "sensor") {
            const val = cap.value as number;
            return (
              <div key={cap.id} className="flex items-center justify-between text-sm">
                <span className="text-zinc-400">{cap.label}</span>
                <span className="font-mono text-zinc-200">
                  {typeof val === "number" ? val.toFixed(1) : val}
                  {cap.unit ? ` ${cap.unit}` : ""}
                </span>
              </div>
            );
          }
          return null;
        })}
      </div>

      {controllable.length > 0 && device.online && (
        <div className="space-y-2 border-t border-zinc-800 pt-2">
          {controllable.map((cap) => {
            if (cap.type === "switch") {
              return (
                <div key={cap.id} className="flex items-center justify-between">
                  <span className="text-sm text-zinc-400">{cap.label}</span>
                  <button
                    role="switch"
                    aria-checked={cap.value as boolean}
                    onClick={() => handleChange(cap.id, !(cap.value as boolean))}
                    className={`relative w-10 h-5 rounded-full transition-colors duration-200 ${
                      cap.value ? "bg-trellis-500" : "bg-zinc-600"
                    }`}
                  >
                    <span
                      className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full transition-all duration-200 shadow-sm ${
                        cap.value ? "translate-x-5" : "translate-x-0"
                      }`}
                    />
                  </button>
                </div>
              );
            }
            if (cap.type === "slider") {
              return (
                <div key={cap.id}>
                  <div className="flex items-center justify-between text-sm mb-1">
                    <span className="text-zinc-400">{cap.label}</span>
                    <span className="text-zinc-300 font-mono text-xs">
                      {cap.value as number}
                      {cap.unit ? ` ${cap.unit}` : ""}
                    </span>
                  </div>
                  <input
                    type="range"
                    min={cap.min ?? 0}
                    max={cap.max ?? 100}
                    value={cap.value as number}
                    onChange={(e) => handleChange(cap.id, Number(e.target.value))}
                    className="w-full h-1.5 bg-zinc-700 rounded-full appearance-none cursor-pointer accent-trellis-500"
                  />
                </div>
              );
            }
            return null;
          })}
        </div>
      )}

      {!device.online && (
        <div className="text-xs text-red-400/70 text-center py-1">
          Device offline
        </div>
      )}

      <button
        onClick={onNavigate}
        className="mt-2 w-full text-center text-xs text-trellis-400 hover:text-trellis-300 transition-colors py-1"
      >
        Open device details
      </button>
    </div>
  );
}

// ─── Floor tab bar ─────────────────────────────────────────────────

function FloorTabs({
  floors,
  activeId,
  onSelect,
  onAdd,
  onRename,
  onDelete,
}: {
  floors: FloorPlanEntry[];
  activeId: number | null;
  onSelect: (id: number) => void;
  onAdd: () => void;
  onRename: (floor: FloorPlanEntry) => void;
  onDelete: (floor: FloorPlanEntry) => void;
}) {
  const [contextMenu, setContextMenu] = useState<{
    floor: FloorPlanEntry;
    x: number;
    y: number;
  } | null>(null);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [contextMenu]);

  return (
    <div className="flex items-center gap-1 px-3 py-1.5 border-b border-zinc-800/50 bg-zinc-900/50 min-h-[36px]">
      <Layers size={13} className="text-zinc-600 mr-1 flex-shrink-0" />
      <div className="flex items-center gap-1 overflow-x-auto scrollbar-none">
        {floors.map((floor) => (
          <button
            key={floor.id}
            onClick={() => onSelect(floor.id)}
            onContextMenu={(e) => {
              e.preventDefault();
              setContextMenu({ floor, x: e.clientX, y: e.clientY });
            }}
            className={`px-3 py-1 text-xs rounded-md whitespace-nowrap transition-all ${
              activeId === floor.id
                ? "bg-trellis-500/15 text-trellis-400 border border-trellis-500/30"
                : "text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/50 border border-transparent"
            }`}
          >
            {floor.name}
          </button>
        ))}
      </div>
      <button
        onClick={onAdd}
        className="flex items-center gap-1 px-2 py-1 text-xs text-zinc-600 hover:text-zinc-300 hover:bg-zinc-800/50 rounded-md transition-colors flex-shrink-0 ml-1"
        title="Add floor"
      >
        <Plus size={12} />
      </button>

      {/* Context menu */}
      {contextMenu && (
        <div
          className="fixed z-50 bg-zinc-900 border border-zinc-700 rounded-lg shadow-xl py-1 min-w-[140px]"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <button
            onClick={(e) => {
              e.stopPropagation();
              onRename(contextMenu.floor);
              setContextMenu(null);
            }}
            className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-zinc-300 hover:bg-zinc-800 transition-colors"
          >
            <Pencil size={11} />
            Rename
          </button>
          {floors.length > 1 && (
            <button
              onClick={(e) => {
                e.stopPropagation();
                onDelete(contextMenu.floor);
                setContextMenu(null);
              }}
              className="flex items-center gap-2 w-full px-3 py-1.5 text-xs text-red-400/80 hover:bg-zinc-800 transition-colors"
            >
              <Trash2 size={11} />
              Delete
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Main floor plan page ──────────────────────────────────────────

export default function FloorPlan() {
  const { devices, initEventListeners } = useDeviceStore();
  const navigate = useNavigate();
  const canvasRef = useRef<HTMLDivElement>(null);

  const [floors, setFloors] = useState<FloorPlanEntry[]>([]);
  const [activeFloorId, setActiveFloorId] = useState<number | null>(null);
  const [positions, setPositions] = useState<DevicePosition[]>([]);
  const [background, setBackground] = useState<string | null>(null);
  const [allPositions, setAllPositions] = useState<DevicePosition[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [popup, setPopup] = useState<{
    deviceId: string;
    screenX: number;
    screenY: number;
  } | null>(null);
  const [dragging, setDragging] = useState<{
    deviceId: string;
    startX: number;
    startY: number;
    origX: number;
    origY: number;
  } | null>(null);
  const [snapToGrid, setSnapToGrid] = useState(false);
  const [compactNodes, setCompactNodes] = useState(false);
  const snapRef = useRef(false);
  snapRef.current = snapToGrid;
  const [renaming, setRenaming] = useState<FloorPlanEntry | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [showAddFloor, setShowAddFloor] = useState(false);
  const [newFloorName, setNewFloorName] = useState("");

  useEffect(() => {
    initEventListeners();
  }, [initEventListeners]);

  // Load floors on mount
  const loadFloors = useCallback(async () => {
    try {
      let floorList = await invoke<FloorPlanEntry[]>("get_floor_plans");
      if (floorList.length === 0) {
        // Auto-create default floor on first visit
        const id = await invoke<number>("create_floor_plan", { name: "Floor 1" });
        floorList = [{ id, name: "Floor 1", sort_order: 0, background: null }];
      }
      setFloors(floorList);
      return floorList;
    } catch (err) {
      console.error("Failed to load floors:", err);
      return [];
    }
  }, []);

  // Load positions for a specific floor
  const loadFloorData = useCallback(async (floorId: number) => {
    try {
      const posRows = await invoke<DevicePosition[]>("get_device_positions", { floorId });
      setPositions(posRows);
    } catch (err) {
      console.error("Failed to load positions:", err);
    }
  }, []);

  // Load all positions (for sidebar: know which devices are placed on any floor)
  const loadAllPositions = useCallback(async () => {
    try {
      const all = await invoke<DevicePosition[]>("get_all_device_positions");
      setAllPositions(all);
    } catch (err) {
      console.error("Failed to load all positions:", err);
    }
  }, []);

  // Initial load
  useEffect(() => {
    (async () => {
      const floorList = await loadFloors();
      await loadAllPositions();
      if (floorList.length > 0) {
        setActiveFloorId(floorList[0].id);
      }
    })();
  }, [loadFloors, loadAllPositions]);

  // Load floor data when active floor changes
  useEffect(() => {
    if (activeFloorId === null) return;
    loadFloorData(activeFloorId);
    // Set background from the active floor
    const floor = floors.find((f) => f.id === activeFloorId);
    setBackground(floor?.background ?? null);
  }, [activeFloorId, floors, loadFloorData]);

  // Reload helper
  const reloadCurrentFloor = useCallback(async () => {
    if (activeFloorId !== null) {
      await loadFloorData(activeFloorId);
    }
    await loadAllPositions();
  }, [activeFloorId, loadFloorData, loadAllPositions]);

  // ─── Floor CRUD ───────────────────────────────────────────────
  const handleAddFloor = async () => {
    const name = newFloorName.trim();
    if (!name) return;
    try {
      const id = await invoke<number>("create_floor_plan", { name });
      setShowAddFloor(false);
      setNewFloorName("");
      const floorList = await loadFloors();
      // Switch to the new floor
      const newFloor = floorList.find((f) => f.id === id);
      if (newFloor) {
        setActiveFloorId(newFloor.id);
      }
    } catch (err) {
      console.error("Failed to create floor:", err);
    }
  };

  const handleRename = async () => {
    if (!renaming) return;
    const name = renameValue.trim();
    if (!name) return;
    try {
      await invoke("update_floor_plan", { id: renaming.id, name, background: null });
      setRenaming(null);
      setRenameValue("");
      await loadFloors();
    } catch (err) {
      console.error("Failed to rename floor:", err);
    }
  };

  const handleDeleteFloor = async (floor: FloorPlanEntry) => {
    if (floors.length <= 1) return;
    try {
      await invoke("delete_floor_plan", { id: floor.id });
      const floorList = await loadFloors();
      await loadAllPositions();
      // Switch to another floor
      if (activeFloorId === floor.id && floorList.length > 0) {
        setActiveFloorId(floorList[0].id);
      }
    } catch (err) {
      console.error("Failed to delete floor:", err);
    }
  };

  // ─── Canvas drag: drop a new device from the sidebar ──────────
  const handleCanvasDragOver = (e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
  };

  const handleCanvasDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    const deviceId = e.dataTransfer.getData("text/plain");
    if (!deviceId || !canvasRef.current || activeFloorId === null) return;

    const rect = canvasRef.current.getBoundingClientRect();
    const rawX = ((e.clientX - rect.left) / rect.width) * 100;
    const rawY = ((e.clientY - rect.top) / rect.height) * 100;
    const cx = clampSnap(rawX, snapToGrid);
    const cy = clampSnap(rawY, snapToGrid);

    // Optimistic update
    setPositions((prev) => {
      const filtered = prev.filter((p) => p.device_id !== deviceId);
      return [...filtered, { device_id: deviceId, floor_id: activeFloorId, x: cx, y: cy }];
    });
    setAllPositions((prev) => {
      const filtered = prev.filter((p) => p.device_id !== deviceId);
      return [...filtered, { device_id: deviceId, floor_id: activeFloorId, x: cx, y: cy }];
    });

    try {
      await invoke("set_device_position", { deviceId, floorId: activeFloorId, x: cx, y: cy });
    } catch (err) {
      console.error("Failed to save position:", err);
      reloadCurrentFloor();
    }
  };

  // ─── Canvas move: drag an already-placed device ───────────────
  const handleNodeDragStart = (deviceId: string, e: React.MouseEvent) => {
    if (!canvasRef.current) return;
    const pos = positions.find((p) => p.device_id === deviceId);
    if (!pos) return;

    setDragging({
      deviceId,
      startX: e.clientX,
      startY: e.clientY,
      origX: pos.x,
      origY: pos.y,
    });
  };

  useEffect(() => {
    if (!dragging) return;

    const handleMove = (e: MouseEvent) => {
      if (!canvasRef.current) return;
      const rect = canvasRef.current.getBoundingClientRect();
      const dx = ((e.clientX - dragging.startX) / rect.width) * 100;
      const dy = ((e.clientY - dragging.startY) / rect.height) * 100;
      const nx = clampSnap(dragging.origX + dx, snapRef.current);
      const ny = clampSnap(dragging.origY + dy, snapRef.current);

      setPositions((prev) =>
        prev.map((p) =>
          p.device_id === dragging.deviceId ? { ...p, x: nx, y: ny } : p
        )
      );
    };

    const handleUp = async (e: MouseEvent) => {
      if (!canvasRef.current || activeFloorId === null) return;
      const rect = canvasRef.current.getBoundingClientRect();
      const dx = ((e.clientX - dragging.startX) / rect.width) * 100;
      const dy = ((e.clientY - dragging.startY) / rect.height) * 100;
      const nx = clampSnap(dragging.origX + dx, snapRef.current);
      const ny = clampSnap(dragging.origY + dy, snapRef.current);

      const dist = Math.abs(dx) + Math.abs(dy);
      if (dist < 0.5) {
        setPopup({
          deviceId: dragging.deviceId,
          screenX: e.clientX,
          screenY: e.clientY,
        });
      }

      setDragging(null);

      try {
        await invoke("set_device_position", {
          deviceId: dragging.deviceId,
          floorId: activeFloorId,
          x: nx,
          y: ny,
        });
      } catch (err) {
        console.error("Failed to save position:", err);
        reloadCurrentFloor();
      }
    };

    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseup", handleUp);
    return () => {
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseup", handleUp);
    };
  }, [dragging, activeFloorId, reloadCurrentFloor]);

  // ─── Remove device from floor plan ────────────────────────────
  const removeFromPlan = async (deviceId: string) => {
    setPositions((prev) => prev.filter((p) => p.device_id !== deviceId));
    setAllPositions((prev) => prev.filter((p) => p.device_id !== deviceId));
    setSelected(null);
    setPopup(null);
    try {
      await invoke("remove_device_position", { deviceId });
    } catch (err) {
      console.error("Failed to remove position:", err);
      reloadCurrentFloor();
    }
  };

  // ─── Background image ─────────────────────────────────────────
  const handleBackgroundUpload = () => {
    if (activeFloorId === null) return;
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file || activeFloorId === null) return;
      const reader = new FileReader();
      reader.onload = async () => {
        const dataUrl = reader.result as string;
        setBackground(dataUrl);
        setFloors((prev) =>
          prev.map((f) => (f.id === activeFloorId ? { ...f, background: dataUrl } : f))
        );
        try {
          await invoke("update_floor_plan", {
            id: activeFloorId,
            name: null,
            background: dataUrl,
          });
        } catch (err) {
          console.error("Failed to save background:", err);
        }
      };
      reader.readAsDataURL(file);
    };
    input.click();
  };

  const clearBackground = async () => {
    if (activeFloorId === null) return;
    setBackground(null);
    setFloors((prev) =>
      prev.map((f) => (f.id === activeFloorId ? { ...f, background: null } : f))
    );
    try {
      await invoke("update_floor_plan", {
        id: activeFloorId,
        name: null,
        background: "",
      });
    } catch (err) {
      console.error("Failed to clear background:", err);
    }
  };

  // ─── Derived ──────────────────────────────────────────────────
  const placedIds = new Set(positions.map((p) => p.device_id));
  const allPlacedIds = new Set(allPositions.map((p) => p.device_id));
  const unplaced = devices.filter((d) => !allPlacedIds.has(d.id));

  const handleCanvasClick = (e: React.MouseEvent) => {
    if (e.target === canvasRef.current || (e.target as HTMLElement).dataset.canvasBg) {
      setSelected(null);
      setPopup(null);
    }
  };

  const handleSidebarDragStart = (e: React.DragEvent, deviceId: string) => {
    e.dataTransfer.setData("text/plain", deviceId);
    e.dataTransfer.effectAllowed = "move";
  };

  return (
    <div className="flex flex-col h-full">
      {/* Floor tab bar */}
      <FloorTabs
        floors={floors}
        activeId={activeFloorId}
        onSelect={(id) => {
          setActiveFloorId(id);
          setSelected(null);
          setPopup(null);
        }}
        onAdd={() => {
          setNewFloorName("");
          setShowAddFloor(true);
        }}
        onRename={(floor) => {
          setRenameValue(floor.name);
          setRenaming(floor);
        }}
        onDelete={handleDeleteFloor}
      />

      <div className="flex flex-1 min-h-0">
        {/* Sidebar: unplaced devices */}
        <div className="w-56 flex-shrink-0 border-r border-zinc-800/50 flex flex-col">
          <div className="p-3 border-b border-zinc-800/50">
            <div className="flex items-center gap-2 mb-1">
              <Map size={15} className="text-trellis-400" />
              <h2 className="text-sm font-semibold text-zinc-300 uppercase tracking-wide">
                Devices
              </h2>
            </div>
            <p className="text-[11px] text-zinc-600">
              Drag onto the canvas to place
            </p>
          </div>

          <div className="flex-1 overflow-y-auto p-2 space-y-1">
            {unplaced.length === 0 && (
              <p className="text-xs text-zinc-600 text-center py-4">
                All devices placed
              </p>
            )}
            {unplaced.map((d) => (
              <UnplacedCard
                key={d.id}
                device={d}
                onDragStart={handleSidebarDragStart}
              />
            ))}
          </div>

          {/* Canvas controls */}
          <div className="p-2 border-t border-zinc-800/50 space-y-1">
            <button
              onClick={() => setSnapToGrid((v) => !v)}
              className={`flex items-center gap-2 w-full px-2.5 py-1.5 text-xs rounded-lg transition-colors ${
                snapToGrid
                  ? "text-trellis-400 bg-trellis-500/10 hover:bg-trellis-500/15"
                  : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"
              }`}
            >
              <Grid3x3 size={13} />
              Snap to grid
            </button>
            <button
              onClick={() => setCompactNodes((v) => !v)}
              className={`flex items-center gap-2 w-full px-2.5 py-1.5 text-xs rounded-lg transition-colors ${
                compactNodes
                  ? "text-trellis-400 bg-trellis-500/10 hover:bg-trellis-500/15"
                  : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"
              }`}
            >
              <Minimize2 size={13} />
              Compact labels
            </button>
            <button
              onClick={handleBackgroundUpload}
              className="flex items-center gap-2 w-full px-2.5 py-1.5 text-xs text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50 rounded-lg transition-colors"
            >
              <ImagePlus size={13} />
              {background ? "Change background" : "Set background"}
            </button>
            {background && (
              <button
                onClick={clearBackground}
                className="flex items-center gap-2 w-full px-2.5 py-1.5 text-xs text-red-400/70 hover:text-red-400 hover:bg-zinc-800/50 rounded-lg transition-colors"
              >
                <X size={13} />
                Clear background
              </button>
            )}
          </div>

          {/* Selected device actions */}
          {selected && placedIds.has(selected) && (
            <div className="p-2 border-t border-zinc-800/50">
              <button
                onClick={() => removeFromPlan(selected)}
                className="flex items-center gap-2 w-full px-2.5 py-1.5 text-xs text-red-400/70 hover:text-red-400 hover:bg-zinc-800/50 rounded-lg transition-colors"
              >
                <Trash2 size={13} />
                Remove from floor plan
              </button>
            </div>
          )}
        </div>

        {/* Canvas area */}
        <div className="flex-1 relative overflow-hidden">
          <div
            ref={canvasRef}
            onDragOver={handleCanvasDragOver}
            onDrop={handleCanvasDrop}
            onClick={handleCanvasClick}
            className="absolute inset-0"
            style={{
              backgroundImage: background
                ? `url(${background})`
                : undefined,
              backgroundSize: "cover",
              backgroundPosition: "center",
              backgroundRepeat: "no-repeat",
            }}
          >
            {/* Grid pattern — always visible when snap is on, otherwise only without background */}
            {(!background || snapToGrid) && (
              <div
                data-canvas-bg="true"
                className="absolute inset-0 pointer-events-none"
                style={{
                  backgroundImage: snapToGrid
                    ? "radial-gradient(circle, rgba(45,212,191,0.25) 1px, transparent 1px)"
                    : "radial-gradient(circle, rgba(113,113,122,0.15) 1px, transparent 1px)",
                  backgroundSize: "32px 32px",
                }}
              />
            )}

            {/* Empty state */}
            {positions.length === 0 && (
              <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
                <div className="text-center">
                  <Map size={48} className="text-zinc-700 mx-auto mb-3" />
                  <p className="text-sm text-zinc-500">
                    Drag devices from the sidebar to place them on your floor plan
                  </p>
                  {devices.length > 0 && !background && (
                    <p className="text-xs text-zinc-600 mt-2">
                      Tip: Set a background image of your room or floor plan
                    </p>
                  )}
                </div>
              </div>
            )}

            {/* Placed device nodes */}
            {positions.map((pos) => {
              const device = devices.find((d) => d.id === pos.device_id);
              if (!device) return null;
              return (
                <PlacedNode
                  key={pos.device_id}
                  device={device}
                  pos={pos}
                  selected={selected === pos.device_id}
                  compact={compactNodes}
                  onSelect={() => setSelected(pos.device_id)}
                  onDragStart={(e) => handleNodeDragStart(pos.device_id, e)}
                />
              );
            })}

            {/* Inline control popup */}
            {popup && (() => {
              const device = devices.find((d) => d.id === popup.deviceId);
              if (!device || !canvasRef.current) return null;
              const rect = canvasRef.current.getBoundingClientRect();
              let px = popup.screenX - rect.left + 10;
              let py = popup.screenY - rect.top - 10;
              if (px + 280 > rect.width) px = rect.width - 290;
              if (py + 200 > rect.height) py = rect.height - 210;
              if (px < 10) px = 10;
              if (py < 10) py = 10;

              return (
                <div
                  style={{
                    position: "absolute",
                    left: px,
                    top: py,
                    zIndex: 30,
                  }}
                >
                  <InlineControl
                    device={device}
                    onClose={() => setPopup(null)}
                    onNavigate={() => navigate(`/device/${device.id}`)}
                  />
                </div>
              );
            })()}
          </div>

          {/* Legend */}
          <div className="absolute bottom-3 right-3 flex items-center gap-3 text-[11px] text-zinc-600 bg-zinc-900/80 backdrop-blur-sm border border-zinc-800/50 rounded-lg px-3 py-1.5">
            <span className="flex items-center gap-1">
              <span className="w-2 h-2 rounded-full bg-emerald-400" />
              Online
            </span>
            <span className="flex items-center gap-1">
              <span className="w-2 h-2 rounded-full bg-zinc-600" />
              Offline
            </span>
            <span className="text-zinc-700">|</span>
            <span>Click node to interact</span>
          </div>

          {/* Device count badge */}
          <div className="absolute top-3 right-3 text-[11px] text-zinc-500 bg-zinc-900/80 backdrop-blur-sm border border-zinc-800/50 rounded-lg px-2.5 py-1">
            {positions.length} / {devices.length} placed
          </div>
        </div>
      </div>

      {/* Add floor modal */}
      {showAddFloor && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
          <div className="bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl p-4 w-80">
            <h3 className="text-sm font-semibold text-zinc-200 mb-3">Add Floor</h3>
            <input
              autoFocus
              value={newFloorName}
              onChange={(e) => setNewFloorName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleAddFloor();
                if (e.key === "Escape") setShowAddFloor(false);
              }}
              placeholder="e.g. Second Floor, Garage, Basement"
              className="w-full px-3 py-2 text-sm bg-zinc-800 border border-zinc-700 rounded-lg text-zinc-200 placeholder-zinc-600 focus:outline-none focus:border-trellis-500 mb-3"
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setShowAddFloor(false)}
                className="px-3 py-1.5 text-xs text-zinc-400 hover:text-zinc-200 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleAddFloor}
                disabled={!newFloorName.trim()}
                className="px-3 py-1.5 text-xs bg-trellis-500/20 text-trellis-400 border border-trellis-500/30 rounded-lg hover:bg-trellis-500/30 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Add
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Rename floor modal */}
      {renaming && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm">
          <div className="bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl p-4 w-80">
            <h3 className="text-sm font-semibold text-zinc-200 mb-3">Rename Floor</h3>
            <input
              autoFocus
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") handleRename();
                if (e.key === "Escape") setRenaming(null);
              }}
              className="w-full px-3 py-2 text-sm bg-zinc-800 border border-zinc-700 rounded-lg text-zinc-200 placeholder-zinc-600 focus:outline-none focus:border-trellis-500 mb-3"
            />
            <div className="flex justify-end gap-2">
              <button
                onClick={() => setRenaming(null)}
                className="px-3 py-1.5 text-xs text-zinc-400 hover:text-zinc-200 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleRename}
                disabled={!renameValue.trim()}
                className="px-3 py-1.5 text-xs bg-trellis-500/20 text-trellis-400 border border-trellis-500/30 rounded-lg hover:bg-trellis-500/30 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Save
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
