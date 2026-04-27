import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import { useDeviceStore } from "@/stores/deviceStore";
import type { SerialPortInfo } from "@/lib/types";
import { generateSketch } from "@/lib/sketchGenerator";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Cpu,
  Trash2,
  Save,
  FolderOpen,
  ToggleLeft,
  Thermometer,
  SlidersHorizontal,
  Palette,
  Type,
  Copy,
  Check,
  Hammer,
  Usb,
  Wifi,
  RefreshCw,
  ChevronDown,
  ChevronRight,
  Terminal as TerminalIcon,
  Loader2,
  Download,
} from "lucide-react";

interface CapabilityDef {
  id: string;
  type: "switch" | "sensor" | "slider" | "color" | "text";
  label: string;
  gpio: string;
  unit: string;
  min: string;
  max: string;
}

const CAP_ICONS = {
  switch: ToggleLeft,
  sensor: Thermometer,
  slider: SlidersHorizontal,
  color: Palette,
  text: Type,
};

const CAP_DEFAULTS: Record<string, Partial<CapabilityDef>> = {
  switch: { gpio: "2", unit: "", min: "", max: "" },
  sensor: { gpio: "", unit: "C", min: "", max: "" },
  slider: { gpio: "25", unit: "", min: "0", max: "100" },
  color: { gpio: "", unit: "", min: "", max: "" },
  text: { gpio: "", unit: "", min: "", max: "" },
};

function highlightLine(line: string): string {
  const trimmed = line.trim();
  if (trimmed.startsWith("//") || trimmed.startsWith("/*") || trimmed.startsWith("*"))
    return "text-zinc-600";
  if (trimmed.startsWith("#include") || trimmed.startsWith("#if") || trimmed.startsWith("#elif") || trimmed.startsWith("#endif"))
    return "text-purple-400/80";
  if (trimmed.startsWith("void ") || trimmed.startsWith("float ") || trimmed.startsWith("bool ") || trimmed.startsWith("const "))
    return "text-blue-400/80";
  if (trimmed.includes('"'))
    return "text-trellis-400/80";
  return "text-zinc-300";
}

interface TemplateDef {
  id: number;
  name: string;
  description: string;
  capabilities: string;
  created_at: string;
}

const BOARD_FQBN: Record<string, string> = {
  esp32: "esp32:esp32:esp32",
  picow: "rp2040:rp2040:rpipicow",
};

export default function FirmwareGenerator() {
  const [deviceName, setDeviceName] = useState("My Device");
  const [board, setBoard] = useState<"esp32" | "picow">("esp32");
  const [capabilities, setCapabilities] = useState<CapabilityDef[]>([]);
  const [copied, setCopied] = useState(false);
  const [templates, setTemplates] = useState<TemplateDef[]>([]);
  const [showTemplates, setShowTemplates] = useState(false);
  const [sketch, setSketch] = useState("");
  const [sketchError, setSketchError] = useState("");

  // Quick Flash state
  const [cliAvailable, setCliAvailable] = useState<string | null>(null);
  const [cliChecked, setCliChecked] = useState(false);
  const [serialPorts, setSerialPorts] = useState<SerialPortInfo[]>([]);
  const [selectedPort, setSelectedPort] = useState("");
  const [compiling, setCompiling] = useState(false);
  const [flashing, setFlashing] = useState(false);
  const [compiled, setCompiled] = useState(false);
  const [buildOutput, setBuildOutput] = useState("");
  const [buildError, setBuildError] = useState(false);
  const [flashExpanded, setFlashExpanded] = useState(true);
  const [depsChecked, setDepsChecked] = useState(false);
  const [missingDeps, setMissingDeps] = useState<string[]>([]);
  const [installingDeps, setInstallingDeps] = useState(false);
  const [elapsedTime, setElapsedTime] = useState(0);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const outputRef = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();
  const { devices } = useDeviceStore();
  const onlineDevices = devices.filter((d) => d.online);

  useEffect(() => { loadTemplates(); }, []);

  // Check arduino-cli, deps, and load serial ports on mount
  useEffect(() => {
    invoke<string>("check_arduino_cli")
      .then((version) => {
        setCliAvailable(version);
        setCliChecked(true);
        checkDeps(board);
      })
      .catch(() => {
        setCliAvailable(null);
        setCliChecked(true);
        setFlashExpanded(false);
      });
    refreshPorts();
  }, []);

  // Re-check deps when board changes
  useEffect(() => {
    if (cliAvailable) {
      setDepsChecked(false);
      checkDeps(board);
    }
  }, [board]);

  // Cleanup timer on unmount
  useEffect(() => {
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, []);

  // Auto-scroll output panel
  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [buildOutput]);

  // Reset compiled state when capabilities or board change
  useEffect(() => {
    setCompiled(false);
  }, [capabilities, board, deviceName]);

  const refreshPorts = () => {
    invoke<SerialPortInfo[]>("list_serial_ports")
      .then((ports) => {
        setSerialPorts(ports);
        if (ports.length > 0 && !selectedPort) {
          setSelectedPort(ports[0].name);
        }
      })
      .catch(() => setSerialPorts([]));
  };

  const startTimer = () => {
    setElapsedTime(0);
    timerRef.current = setInterval(() => setElapsedTime((t) => t + 1), 1000);
  };
  const stopTimer = () => {
    if (timerRef.current) {
      clearInterval(timerRef.current);
      timerRef.current = null;
    }
  };

  const checkDeps = async (boardType: string) => {
    try {
      const result = await invoke<{
        core_installed: boolean;
        core_name: string;
        trellis_installed: boolean;
        arduinojson_installed: boolean;
        websockets_installed: boolean;
      }>("check_arduino_deps", { board: boardType });

      const missing: string[] = [];
      if (!result.core_installed) missing.push(result.core_name);
      if (!result.trellis_installed) missing.push("Trellis");
      if (!result.arduinojson_installed) missing.push("ArduinoJson");
      if (!result.websockets_installed) missing.push("WebSockets");
      setMissingDeps(missing);
      setDepsChecked(true);
    } catch {
      setDepsChecked(true);
    }
  };

  const handleInstallDeps = async () => {
    setInstallingDeps(true);
    setBuildOutput("");
    try {
      const output = await invoke<string>("install_arduino_deps", {
        deps: missingDeps,
      });
      setBuildOutput(output);
      setMissingDeps([]);
      checkDeps(board);
    } catch (err) {
      setBuildOutput(String(err));
      setBuildError(true);
    } finally {
      setInstallingDeps(false);
    }
  };

  const loadTemplates = async () => {
    try {
      const t = await invoke<TemplateDef[]>("get_templates");
      setTemplates(t);
    } catch (err) {
      console.error("Failed to load templates:", err);
    }
  };

  const saveAsTemplate = async () => {
    if (capabilities.length === 0) return;
    const name = prompt("Template name:", deviceName);
    if (!name) return;
    await invoke("create_template", {
      name,
      description: `${capabilities.length} capabilities`,
      capabilities: JSON.stringify(capabilities),
    });
    loadTemplates();
  };

  const loadTemplate = (t: TemplateDef) => {
    try {
      const caps = JSON.parse(t.capabilities) as CapabilityDef[];
      setCapabilities(caps);
      setDeviceName(t.name);
      setShowTemplates(false);
    } catch (err) {
      console.error("Failed to parse template:", err);
    }
  };

  const deleteTemplate = async (id: number) => {
    if (!confirm("Delete this template?")) return;
    await invoke("delete_template", { id });
    loadTemplates();
  };

  const addCapability = (type: CapabilityDef["type"]) => {
    const count = capabilities.filter((c) => c.type === type).length;
    const defaults = CAP_DEFAULTS[type];
    setCapabilities([
      ...capabilities,
      {
        id: `${type}_${count}`,
        type,
        label: `${type.charAt(0).toUpperCase() + type.slice(1)} ${count + 1}`,
        gpio: defaults.gpio || "",
        unit: defaults.unit || "",
        min: defaults.min || "",
        max: defaults.max || "",
      },
    ]);
  };

  const updateCapability = (index: number, field: string, value: string) => {
    setCapabilities(
      capabilities.map((c, i) =>
        i === index ? { ...c, [field]: value } : c,
      ),
    );
  };

  const removeCapability = (index: number) => {
    setCapabilities(capabilities.filter((_, i) => i !== index));
  };

  // Sketch is generated by the backend (single source of truth — see
  // app/src-tauri/src/sketch_gen.rs). Re-generate on any dependency change.
  useEffect(() => {
    let cancelled = false;
    generateSketch(deviceName, board, capabilities)
      .then((s) => {
        if (cancelled) return;
        setSketch(s);
        setSketchError("");
      })
      .catch((e) => {
        if (cancelled) return;
        setSketch("");
        setSketchError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [deviceName, board, capabilities]);

  const copySketch = () => {
    navigator.clipboard.writeText(sketch);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleCompile = async () => {
    setCompiling(true);
    setBuildOutput("");
    setBuildError(false);
    setCompiled(false);
    startTimer();
    try {
      const fresh = await generateSketch(deviceName, board, capabilities);
      const output = await invoke<string>("compile_sketch", { sketch: fresh, board });
      setBuildOutput(output);
      setCompiled(true);
    } catch (err) {
      setBuildOutput(String(err));
      setBuildError(true);
    } finally {
      setCompiling(false);
      stopTimer();
    }
  };

  const handleFlash = async () => {
    if (!selectedPort) return;
    setFlashing(true);
    setBuildOutput("");
    setBuildError(false);
    startTimer();
    try {
      const output = await invoke<string>("flash_sketch", {
        board,
        port: selectedPort,
      });
      setBuildOutput(output);
    } catch (err) {
      setBuildOutput(String(err));
      setBuildError(true);
    } finally {
      setFlashing(false);
      stopTimer();
    }
  };

  const handleCompileAndFlash = async () => {
    if (!selectedPort) return;
    setCompiling(true);
    setBuildOutput("");
    setBuildError(false);
    setCompiled(false);
    startTimer();
    try {
      const fresh = await generateSketch(deviceName, board, capabilities);
      const compileOutput = await invoke<string>("compile_sketch", {
        sketch: fresh,
        board,
      });
      setBuildOutput(
        compileOutput + "\n\nUploading to " + selectedPort + "...\n",
      );
      setCompiled(true);
      setCompiling(false);
      setFlashing(true);
      const flashOutput = await invoke<string>("flash_sketch", {
        board,
        port: selectedPort,
      });
      setBuildOutput(compileOutput + "\n\n" + flashOutput);
    } catch (err) {
      setBuildOutput((prev) => prev + "\n" + String(err));
      setBuildError(true);
    } finally {
      setCompiling(false);
      setFlashing(false);
      stopTimer();
    }
  };

  return (
    <div className="flex gap-6 h-full">
      {/* Left: Configuration */}
      <div className="w-80 flex-shrink-0 overflow-auto">
        <h1 className="text-xl font-bold text-zinc-100 mb-1">
          New Device
        </h1>
        <p className="text-sm text-zinc-500 mb-4">
          Pick capabilities and get a ready-to-flash Arduino sketch.
        </p>

        <div className="flex gap-1.5 mb-5">
          <button
            onClick={() => setShowTemplates(!showTemplates)}
            className="flex items-center gap-1.5 px-2.5 py-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700/50 rounded-lg text-xs text-zinc-300"
          >
            <FolderOpen size={12} /> Templates {templates.length > 0 && `(${templates.length})`}
          </button>
          <button
            onClick={saveAsTemplate}
            disabled={capabilities.length === 0}
            className="flex items-center gap-1.5 px-2.5 py-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700/50 rounded-lg text-xs text-zinc-300 disabled:opacity-30"
          >
            <Save size={12} /> Save as template
          </button>
        </div>

        {showTemplates && templates.length > 0 && (
          <div className="mb-4 p-3 bg-zinc-900 border border-zinc-800 rounded-xl space-y-1.5">
            {templates.map((t) => (
              <div key={t.id} className="flex items-center justify-between p-2 hover:bg-zinc-800/50 rounded-lg">
                <button onClick={() => loadTemplate(t)} className="text-left flex-1">
                  <p className="text-xs text-zinc-300 font-medium">{t.name}</p>
                  <p className="text-[11px] text-zinc-600">{t.description}</p>
                </button>
                <button onClick={() => deleteTemplate(t.id)} className="p-1 text-zinc-600 hover:text-red-400">
                  <Trash2 size={11} />
                </button>
              </div>
            ))}
          </div>
        )}

        <div className="space-y-4">
          <div>
            <label className="block text-xs text-zinc-400 mb-1">
              Device Name
            </label>
            <input
              type="text"
              value={deviceName}
              onChange={(e) => setDeviceName(e.target.value)}
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
            />
          </div>

          <div>
            <label className="block text-xs text-zinc-400 mb-1">Board</label>
            <select
              value={board}
              onChange={(e) => setBoard(e.target.value as "esp32" | "picow")}
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
            >
              <option value="esp32">ESP32</option>
              <option value="picow">Raspberry Pi Pico W</option>
            </select>
          </div>

          <div>
            <label className="block text-xs text-zinc-400 mb-2">
              Add Capabilities
            </label>
            <div className="flex flex-wrap gap-1.5">
              {(
                Object.keys(CAP_ICONS) as Array<keyof typeof CAP_ICONS>
              ).map((type) => {
                const Icon = CAP_ICONS[type];
                return (
                  <button
                    key={type}
                    onClick={() => addCapability(type)}
                    className="flex items-center gap-1.5 px-2.5 py-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-xs text-zinc-300 transition-colors"
                  >
                    <Icon size={12} />
                    {type}
                  </button>
                );
              })}
            </div>
          </div>

          {capabilities.length > 0 && (
            <div className="space-y-2">
              <label className="block text-xs text-zinc-400">
                Capabilities ({capabilities.length})
              </label>
              {capabilities.map((cap, i) => {
                const Icon = CAP_ICONS[cap.type];
                return (
                  <div
                    key={i}
                    className="p-3 bg-zinc-800/50 border border-zinc-700/50 rounded-lg"
                  >
                    <div className="flex items-center justify-between mb-2">
                      <span className="flex items-center gap-1.5 text-xs text-zinc-400">
                        <Icon size={12} />
                        {cap.type}
                      </span>
                      <button
                        onClick={() => removeCapability(i)}
                        className="p-0.5 text-zinc-600 hover:text-red-400 transition-colors"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                    <div className="space-y-1.5">
                      <input
                        type="text"
                        value={cap.label}
                        onChange={(e) =>
                          updateCapability(i, "label", e.target.value)
                        }
                        placeholder="Label"
                        className="w-full bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-300"
                      />
                      <div className="flex gap-1.5">
                        {(cap.type === "switch" || cap.type === "slider") && (
                          <input
                            type="text"
                            value={cap.gpio}
                            onChange={(e) =>
                              updateCapability(i, "gpio", e.target.value)
                            }
                            placeholder="GPIO"
                            className="w-16 bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-300"
                          />
                        )}
                        {cap.type === "sensor" && (
                          <input
                            type="text"
                            value={cap.unit}
                            onChange={(e) =>
                              updateCapability(i, "unit", e.target.value)
                            }
                            placeholder="Unit"
                            className="w-16 bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-300"
                          />
                        )}
                        {cap.type === "slider" && (
                          <>
                            <input
                              type="text"
                              value={cap.min}
                              onChange={(e) =>
                                updateCapability(i, "min", e.target.value)
                              }
                              placeholder="Min"
                              className="w-14 bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-300"
                            />
                            <input
                              type="text"
                              value={cap.max}
                              onChange={(e) =>
                                updateCapability(i, "max", e.target.value)
                              }
                              placeholder="Max"
                              className="w-14 bg-zinc-900 border border-zinc-700 rounded px-2 py-1 text-xs text-zinc-300"
                            />
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {/* Right: Generated code + Quick Flash */}
      <div className="flex-1 flex flex-col min-w-0">
        <div className="flex items-center justify-between mb-2">
          <h2 className="text-sm font-semibold text-zinc-400 flex items-center gap-2">
            <Cpu size={14} />
            Generated Sketch
          </h2>
          <div className="flex items-center gap-2">
            <button
              onClick={async () => {
                const filePath = await save({
                  defaultPath: `${deviceName.replace(/\s+/g, "_")}.ino`,
                  filters: [
                    { name: "Arduino Sketch", extensions: ["ino"] },
                  ],
                });
                if (filePath) {
                  const { writeTextFile } = await import(
                    "@tauri-apps/plugin-fs"
                  );
                  await writeTextFile(filePath, sketch);
                }
              }}
              disabled={!sketch || !!sketchError}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-xs text-zinc-300 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            >
              <Save size={12} />
              Save .ino
            </button>
            <button
              onClick={copySketch}
              disabled={!sketch || !!sketchError}
              className="flex items-center gap-1.5 px-3 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
            >
              {copied ? <Check size={12} /> : <Copy size={12} />}
              {copied ? "Copied!" : "Copy to clipboard"}
            </button>
          </div>
        </div>
        <div className="flex-1 bg-zinc-950 border border-zinc-800 rounded-xl p-4 overflow-auto font-mono text-xs leading-5 select-text min-h-0">
          {sketchError ? (
            <div className="text-amber-400">
              <div className="text-zinc-300 mb-2">Sketch can&apos;t be generated:</div>
              <div className="whitespace-pre-wrap break-words">{sketchError}</div>
            </div>
          ) : (
            sketch.split("\n").map((line, i) => (
              <div key={i} className={highlightLine(line)}>
                {line || "\u00A0"}
              </div>
            ))
          )}
        </div>

        {/* Quick Flash Panel */}
        <div className="mt-3 bg-zinc-900 border border-zinc-800 rounded-xl overflow-hidden flex-shrink-0">
          {/* Collapsible header */}
          <button
            onClick={() => setFlashExpanded(!flashExpanded)}
            className="w-full flex items-center justify-between px-4 py-2.5 hover:bg-zinc-800/50 transition-colors"
          >
            <span className="flex items-center gap-2 text-sm font-semibold text-zinc-300">
              <TerminalIcon size={14} />
              Quick Flash
            </span>
            <div className="flex items-center gap-2">
              {cliChecked && (
                <span className={`text-[11px] ${cliAvailable ? "text-trellis-400" : "text-zinc-600"}`}>
                  {cliAvailable ? `arduino-cli ${cliAvailable}` : "arduino-cli not found"}
                </span>
              )}
              {flashExpanded ? <ChevronDown size={14} className="text-zinc-500" /> : <ChevronRight size={14} className="text-zinc-500" />}
            </div>
          </button>

          {flashExpanded && (
            <div className="px-4 pb-4 space-y-3">
              {/* CLI not installed warning */}
              {cliChecked && !cliAvailable && (
                <div className="flex items-center gap-2 p-2.5 bg-zinc-950 border border-zinc-800 rounded-lg text-xs text-zinc-500">
                  <span>
                    arduino-cli is required to compile and flash.{" "}
                    <a
                      href="https://arduino.github.io/arduino-cli/installation/"
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-trellis-400 hover:underline"
                    >
                      Install instructions
                    </a>
                  </span>
                </div>
              )}

              {/* Missing dependencies banner */}
              {depsChecked && missingDeps.length > 0 && (
                <div className="flex items-center justify-between p-2.5 bg-amber-500/5 border border-amber-500/20 rounded-lg">
                  <div className="text-xs text-amber-400">
                    Missing: {missingDeps.join(", ")}
                  </div>
                  <button
                    onClick={handleInstallDeps}
                    disabled={installingDeps}
                    className="flex items-center gap-1 px-2 py-1 bg-amber-500/20 hover:bg-amber-500/30 text-amber-400 rounded text-xs transition-colors"
                  >
                    {installingDeps ? (
                      <Loader2 size={10} className="animate-spin" />
                    ) : (
                      <Download size={10} />
                    )}
                    {installingDeps ? "Installing..." : "Install"}
                  </button>
                </div>
              )}

              {/* Controls row */}
              {cliAvailable && (
                <div className="flex items-end gap-3">
                  {/* Port selector */}
                  <div className="flex-1 min-w-0">
                    <label className="block text-[11px] text-zinc-500 mb-1">Serial Port</label>
                    <div className="flex gap-1.5">
                      <select
                        value={selectedPort}
                        onChange={(e) => setSelectedPort(e.target.value)}
                        className="flex-1 min-w-0 bg-zinc-800 border border-zinc-700 rounded-lg px-2.5 py-1.5 text-xs text-zinc-300 truncate"
                      >
                        {serialPorts.length === 0 && (
                          <option value="">No ports found</option>
                        )}
                        {serialPorts.map((p) => (
                          <option key={p.name} value={p.name}>
                            {p.name} ({p.port_type})
                          </option>
                        ))}
                      </select>
                      <button
                        onClick={refreshPorts}
                        className="p-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-zinc-400 transition-colors"
                        title="Refresh ports"
                      >
                        <RefreshCw size={12} />
                      </button>
                    </div>
                  </div>

                  {/* Board FQBN hint */}
                  <div className="flex-shrink-0">
                    <label className="block text-[11px] text-zinc-500 mb-1">Board</label>
                    <div className="px-2.5 py-1.5 bg-zinc-800/50 border border-zinc-700/50 rounded-lg text-[11px] text-zinc-500 font-mono">
                      {BOARD_FQBN[board]}
                    </div>
                  </div>
                </div>
              )}

              {/* Action buttons */}
              {cliAvailable && (
                <div className="flex items-center gap-2">
                  <button
                    onClick={handleCompile}
                    disabled={compiling || flashing}
                    className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-xs text-zinc-300 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    {compiling ? <Loader2 size={12} className="animate-spin" /> : <Hammer size={12} />}
                    {compiling ? "Compiling..." : "Compile"}
                  </button>
                  <button
                    onClick={handleCompileAndFlash}
                    disabled={compiling || flashing || !selectedPort}
                    className="flex items-center gap-1.5 px-3 py-1.5 bg-trellis-500/20 hover:bg-trellis-500/30 border border-trellis-500/30 rounded-lg text-xs text-trellis-400 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                    title="Compile then flash in one step"
                  >
                    <Hammer size={12} />
                    Compile & Flash
                  </button>
                  <button
                    onClick={handleFlash}
                    disabled={!compiled || !selectedPort || flashing || compiling}
                    className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-xs text-zinc-300 transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
                    title={!compiled ? "Compile first" : !selectedPort ? "Select a port" : "Flash via USB"}
                  >
                    {flashing ? <Loader2 size={12} className="animate-spin" /> : <Usb size={12} />}
                    {flashing ? "Flashing..." : "Flash USB"}
                  </button>
                  {onlineDevices.length > 0 && (
                    <button
                      onClick={() => navigate("/ota")}
                      className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-xs text-zinc-300 transition-colors"
                      title="Flash via OTA to online devices"
                    >
                      <Wifi size={12} />
                      Flash OTA ({onlineDevices.length})
                    </button>
                  )}
                  {(compiling || flashing) && elapsedTime > 0 && (
                    <span className="text-[11px] text-zinc-500 ml-2">
                      {elapsedTime}s
                    </span>
                  )}
                </div>
              )}

              {/* Output panel */}
              {buildOutput && (
                <div
                  ref={outputRef}
                  className="max-h-[200px] overflow-auto bg-zinc-950 border border-zinc-800 rounded-lg p-3 font-mono text-xs leading-relaxed select-text"
                >
                  <pre className={`whitespace-pre-wrap break-words ${buildError ? "text-red-400" : compiled ? "text-trellis-400" : "text-zinc-300"}`}>
                    {buildOutput}
                  </pre>
                </div>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
