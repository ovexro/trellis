import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import { useDeviceStore } from "@/stores/deviceStore";
import { generateSketch } from "@/lib/sketchGenerator";
import type { SerialPortInfo } from "@/lib/types";

interface StarterCapability {
  id: string;
  type: "switch" | "sensor" | "slider" | "color" | "text";
  label: string;
  gpio: string;
  unit: string;
  min: string;
  max: string;
}

interface StarterTemplate {
  id: string;
  name: string;
  description: string;
  icon: string;
  board: "esp32" | "picow";
  author?: string;
  capabilities: StarterCapability[];
}
import {
  Lightbulb,
  Thermometer,
  Zap,
  CloudSun,
  Sprout,
  Check,
  ChevronRight,
  ChevronLeft,
  Loader2,
  Download,
  RefreshCw,
  Usb,
  Hammer,
  Rocket,
  AlertTriangle,
  Sparkles,
  ExternalLink,
  Trash2,
  ToggleLeft,
  SlidersHorizontal,
  Palette,
  Type,
  Radar,
} from "lucide-react";

const TEMPLATE_ICONS: Record<string, React.ElementType> = {
  lightbulb: Lightbulb,
  thermometer: Thermometer,
  zap: Zap,
  "cloud-sun": CloudSun,
  sprout: Sprout,
};

const CAP_TYPE_ICONS: Record<string, React.ElementType> = {
  switch: ToggleLeft,
  sensor: Thermometer,
  slider: SlidersHorizontal,
  color: Palette,
  text: Type,
};

const STEPS = ["Welcome", "Template", "Flash", "Done"] as const;

export default function GetStarted() {
  const [step, setStep] = useState(0);
  const navigate = useNavigate();
  const { devices } = useDeviceStore();

  // Step 1: Prerequisites
  const [cliVersion, setCliVersion] = useState<string | null>(null);
  const [cliChecked, setCliChecked] = useState(false);
  const [depsChecked, setDepsChecked] = useState(false);
  const [depsError, setDepsError] = useState(false);
  const [missingDeps, setMissingDeps] = useState<string[]>([]);
  const [installingDeps, setInstallingDeps] = useState(false);
  const [installOutput, setInstallOutput] = useState("");

  // Step 2: Template selection
  const [selectedTemplate, setSelectedTemplate] =
    useState<StarterTemplate | null>(null);
  const [starterTemplates, setStarterTemplates] = useState<StarterTemplate[]>([]);

  // Step 3: Configure & Flash
  const [deviceName, setDeviceName] = useState("My Device");
  const [board, setBoard] = useState<"esp32" | "picow">("esp32");
  const [capabilities, setCapabilities] = useState<StarterCapability[]>([]);
  const [serialPorts, setSerialPorts] = useState<SerialPortInfo[]>([]);
  const [selectedPort, setSelectedPort] = useState("");
  const [compiling, setCompiling] = useState(false);
  const [flashing, setFlashing] = useState(false);
  const [buildOutput, setBuildOutput] = useState("");
  const [buildError, setBuildError] = useState(false);
  const [flashSuccess, setFlashSuccess] = useState(false);
  const [elapsedTime, setElapsedTime] = useState(0);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const outputRef = useRef<HTMLDivElement>(null);

  // Step 4: Device discovery watch — snapshot captured when entering Done step
  const [deviceSnapshot, setDeviceSnapshot] = useState<Set<string>>(new Set());
  const newDevice =
    step === 3
      ? devices.find(
          (d) => d.online && !deviceSnapshot.has(d.id),
        )
      : undefined;

  // Check arduino-cli on mount
  useEffect(() => {
    invoke<string>("check_arduino_cli")
      .then((version) => {
        setCliVersion(version);
        setCliChecked(true);
      })
      .catch(() => {
        setCliVersion(null);
        setCliChecked(true);
      });
  }, []);

  // Load curated starter templates from the bundled marketplace catalog
  // (single source of truth — same set the FirmwareGenerator and :9090
  // Sketch tab read).
  useEffect(() => {
    invoke<StarterTemplate[]>("get_marketplace_templates_command")
      .then(setStarterTemplates)
      .catch(() => setStarterTemplates([]));
  }, []);

  // Check deps when board changes and cli is available
  useEffect(() => {
    if (cliVersion) {
      setDepsChecked(false);
      checkDeps(board);
    }
  }, [board, cliVersion]);

  // Auto-scroll output
  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [buildOutput]);

  // Load serial ports when entering step 3
  useEffect(() => {
    if (step === 2) {
      refreshPorts();
    }
  }, [step]);

  // Cleanup timer
  useEffect(() => {
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, []);

  const checkDeps = async (boardType: string) => {
    setDepsError(false);
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
      setDepsError(true);
      setDepsChecked(true);
    }
  };

  const handleInstallDeps = async () => {
    setInstallingDeps(true);
    setInstallOutput("");
    try {
      const output = await invoke<string>("install_arduino_deps", {
        deps: missingDeps,
      });
      setInstallOutput(output);
      setMissingDeps([]);
      checkDeps(board);
    } catch (err) {
      setInstallOutput(String(err));
    } finally {
      setInstallingDeps(false);
    }
  };

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

  const selectTemplate = (t: StarterTemplate) => {
    setSelectedTemplate(t);
    setDeviceName(t.name);
    setBoard(t.board);
    setCapabilities([...t.capabilities]);
    // Clear stale build state from a previous attempt
    setBuildOutput("");
    setBuildError(false);
    setFlashSuccess(false);
  };

  const handleCompileAndFlash = async () => {
    if (!selectedPort) return;
    setCompiling(true);
    setBuildOutput("");
    setBuildError(false);
    setFlashSuccess(false);
    startTimer();
    try {
      const sketch = await generateSketch(deviceName, board, capabilities);
      const compileOutput = await invoke<string>("compile_sketch", {
        sketch,
        board,
      });
      setBuildOutput(
        compileOutput + "\n\nUploading to " + selectedPort + "...\n",
      );
      setCompiling(false);
      setFlashing(true);
      const flashOutput = await invoke<string>("flash_sketch", {
        board,
        port: selectedPort,
      });
      setBuildOutput(compileOutput + "\n\n" + flashOutput);
      setFlashSuccess(true);
    } catch (err) {
      setBuildOutput((prev) => prev + "\n" + String(err));
      setBuildError(true);
    } finally {
      setCompiling(false);
      setFlashing(false);
      stopTimer();
    }
  };

  const markOnboardingDone = async () => {
    try {
      await invoke("set_setting", {
        key: "onboarding_completed",
        value: "true",
      });
    } catch {
      // non-critical
    }
  };

  const finishOnboarding = async () => {
    await markOnboardingDone();
    navigate("/");
  };

  const updateCapability = (
    index: number,
    field: string,
    value: string,
  ) => {
    setCapabilities(
      capabilities.map((c, i) =>
        i === index ? { ...c, [field]: value } : c,
      ),
    );
  };

  const removeCapability = (index: number) => {
    setCapabilities(capabilities.filter((_, i) => i !== index));
  };

  const prereqsReady = cliChecked && cliVersion && depsChecked && !depsError && missingDeps.length === 0;

  // ── Step renderers ──

  const renderWelcome = () => (
    <div className="max-w-2xl mx-auto text-center">
      <div className="w-16 h-16 bg-trellis-500/15 rounded-2xl flex items-center justify-center mx-auto mb-6">
        <Sparkles size={32} className="text-trellis-400" />
      </div>
      <h1 className="text-2xl font-bold text-zinc-100 mb-2">
        Welcome to Trellis
      </h1>
      <p className="text-sm text-zinc-400 mb-8 max-w-md mx-auto">
        Let's get your first device up and running. This wizard will guide you
        through picking a template, flashing your board, and seeing it appear on
        the dashboard.
      </p>

      <div className="bg-zinc-900/50 border border-zinc-800 rounded-xl p-6 text-left space-y-4 mb-8">
        <h2 className="text-sm font-semibold text-zinc-200 mb-3">
          Prerequisites
        </h2>

        {/* arduino-cli check */}
        <div className="flex items-center gap-3">
          <div
            className={`w-7 h-7 rounded-lg flex items-center justify-center ${
              !cliChecked
                ? "bg-zinc-800"
                : cliVersion
                  ? "bg-emerald-500/15"
                  : "bg-red-500/15"
            }`}
          >
            {!cliChecked ? (
              <Loader2 size={14} className="text-zinc-500 animate-spin" />
            ) : cliVersion ? (
              <Check size={14} className="text-emerald-400" />
            ) : (
              <AlertTriangle size={14} className="text-red-400" />
            )}
          </div>
          <div className="flex-1">
            <p className="text-sm text-zinc-300">arduino-cli</p>
            {cliChecked && cliVersion && (
              <p className="text-xs text-zinc-500">v{cliVersion}</p>
            )}
            {cliChecked && !cliVersion && (
              <p className="text-xs text-red-400/80">
                Not found.{" "}
                <a
                  href="https://arduino.github.io/arduino-cli/installation/"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-trellis-400 hover:underline inline-flex items-center gap-0.5"
                >
                  Install instructions <ExternalLink size={10} />
                </a>
              </p>
            )}
          </div>
        </div>

        {/* Dependencies check */}
        {cliVersion && (
          <div className="flex items-center gap-3">
            <div
              className={`w-7 h-7 rounded-lg flex items-center justify-center ${
                !depsChecked
                  ? "bg-zinc-800"
                  : depsError
                    ? "bg-red-500/15"
                    : missingDeps.length === 0
                      ? "bg-emerald-500/15"
                      : "bg-amber-500/15"
              }`}
            >
              {!depsChecked ? (
                <Loader2 size={14} className="text-zinc-500 animate-spin" />
              ) : depsError ? (
                <AlertTriangle size={14} className="text-red-400" />
              ) : missingDeps.length === 0 ? (
                <Check size={14} className="text-emerald-400" />
              ) : (
                <Download size={14} className="text-amber-400" />
              )}
            </div>
            <div className="flex-1">
              <p className="text-sm text-zinc-300">Board core & libraries</p>
              {depsChecked && depsError && (
                <p className="text-xs text-red-400/80">
                  Could not check dependencies
                </p>
              )}
              {depsChecked && !depsError && missingDeps.length === 0 && (
                <p className="text-xs text-zinc-500">All installed</p>
              )}
              {depsChecked && !depsError && missingDeps.length > 0 && (
                <p className="text-xs text-amber-400/80">
                  Missing: {missingDeps.join(", ")}
                </p>
              )}
            </div>
            {depsChecked && depsError && (
              <button
                onClick={() => checkDeps(board)}
                className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-700/50 hover:bg-zinc-700 text-zinc-300 rounded-lg text-xs transition-colors"
              >
                <RefreshCw size={12} />
                Retry
              </button>
            )}
            {depsChecked && !depsError && missingDeps.length > 0 && (
              <button
                onClick={handleInstallDeps}
                disabled={installingDeps}
                className="flex items-center gap-1.5 px-3 py-1.5 bg-amber-500/20 hover:bg-amber-500/30 text-amber-400 rounded-lg text-xs transition-colors"
              >
                {installingDeps ? (
                  <Loader2 size={12} className="animate-spin" />
                ) : (
                  <Download size={12} />
                )}
                {installingDeps ? "Installing..." : "Install"}
              </button>
            )}
          </div>
        )}

        {/* USB device */}
        <div className="flex items-center gap-3">
          <div className="w-7 h-7 rounded-lg flex items-center justify-center bg-zinc-800">
            <Usb size={14} className="text-zinc-400" />
          </div>
          <div className="flex-1">
            <p className="text-sm text-zinc-300">
              ESP32 or Pico W connected via USB
            </p>
            <p className="text-xs text-zinc-500">
              Plug in your board before the flash step
            </p>
          </div>
        </div>

        {installOutput && (
          <div className="mt-3 max-h-32 overflow-auto bg-zinc-950 border border-zinc-800 rounded-lg p-3 font-mono text-xs text-zinc-400">
            <pre className="whitespace-pre-wrap break-words">
              {installOutput}
            </pre>
          </div>
        )}
      </div>

      {devices.length > 0 && (
        <button
          onClick={finishOnboarding}
          className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors"
        >
          Already have devices? Skip to dashboard &rarr;
        </button>
      )}
    </div>
  );

  const renderTemplateSelection = () => (
    <div className="max-w-3xl mx-auto">
      <h2 className="text-lg font-bold text-zinc-100 mb-1 text-center">
        Pick a starter template
      </h2>
      <p className="text-sm text-zinc-500 mb-6 text-center">
        Choose a project that matches your hardware setup. You can customize it
        in the next step.
      </p>

      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {starterTemplates.map((t) => {
          const Icon = TEMPLATE_ICONS[t.icon] || Sparkles;
          const isSelected = selectedTemplate?.id === t.id;
          return (
            <button
              key={t.id}
              onClick={() => selectTemplate(t)}
              className={`text-left p-4 rounded-xl border transition-all duration-150 ${
                isSelected
                  ? "bg-trellis-500/10 border-trellis-500/40 ring-1 ring-trellis-500/20"
                  : "bg-zinc-900/50 border-zinc-800 hover:border-zinc-700 hover:bg-zinc-800/50"
              }`}
            >
              <div className="flex items-center gap-2.5 mb-2.5">
                <div
                  className={`w-9 h-9 rounded-lg flex items-center justify-center ${
                    isSelected ? "bg-trellis-500/20" : "bg-zinc-800"
                  }`}
                >
                  <Icon
                    size={18}
                    className={
                      isSelected ? "text-trellis-400" : "text-zinc-400"
                    }
                  />
                </div>
                <div>
                  <p
                    className={`text-sm font-medium ${
                      isSelected ? "text-trellis-300" : "text-zinc-200"
                    }`}
                  >
                    {t.name}
                  </p>
                </div>
              </div>
              <p className="text-xs text-zinc-500 mb-3">{t.description}</p>
              <div className="flex flex-wrap gap-1">
                {t.capabilities.map((c) => {
                  const CIcon = CAP_TYPE_ICONS[c.type] || Sparkles;
                  return (
                    <span
                      key={c.id}
                      className="inline-flex items-center gap-1 px-1.5 py-0.5 bg-zinc-800/80 rounded text-[10px] text-zinc-500"
                    >
                      <CIcon size={9} />
                      {c.label}
                    </span>
                  );
                })}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );

  const renderConfigureAndFlash = () => {
    return (
      <div className="max-w-2xl mx-auto">
        <h2 className="text-lg font-bold text-zinc-100 mb-1 text-center">
          Configure & Flash
        </h2>
        <p className="text-sm text-zinc-500 mb-6 text-center">
          Customize your device, then compile and flash to your board.
        </p>

        <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
          {/* Left: Configuration */}
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
                onChange={(e) =>
                  setBoard(e.target.value as "esp32" | "picow")
                }
                className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
              >
                <option value="esp32">ESP32</option>
                <option value="picow">Raspberry Pi Pico W</option>
              </select>
            </div>

            <div>
              <label className="block text-xs text-zinc-400 mb-1">
                Serial Port
              </label>
              <div className="flex gap-1.5">
                <select
                  value={selectedPort}
                  onChange={(e) => setSelectedPort(e.target.value)}
                  className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
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
                  className="p-2 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-lg text-zinc-400 transition-colors"
                  title="Refresh ports"
                >
                  <RefreshCw size={14} />
                </button>
              </div>
            </div>

            {/* Capabilities list */}
            <div>
              <label className="block text-xs text-zinc-400 mb-2">
                Capabilities ({capabilities.length})
              </label>
              <div className="space-y-1.5 max-h-48 overflow-auto">
                {capabilities.map((cap, i) => {
                  const CIcon = CAP_TYPE_ICONS[cap.type] || Sparkles;
                  return (
                    <div
                      key={i}
                      className="flex items-center gap-2 p-2 bg-zinc-800/50 border border-zinc-700/50 rounded-lg"
                    >
                      <CIcon size={12} className="text-zinc-500 flex-shrink-0" />
                      <input
                        type="text"
                        value={cap.label}
                        onChange={(e) =>
                          updateCapability(i, "label", e.target.value)
                        }
                        className="flex-1 bg-transparent text-xs text-zinc-300 outline-none"
                      />
                      <span className="text-[10px] text-zinc-600">
                        {cap.type}
                      </span>
                      <button
                        onClick={() => removeCapability(i)}
                        className="p-0.5 text-zinc-600 hover:text-red-400 transition-colors"
                      >
                        <Trash2 size={11} />
                      </button>
                    </div>
                  );
                })}
              </div>
            </div>
          </div>

          {/* Right: Flash controls */}
          <div className="space-y-4">
            <button
              onClick={handleCompileAndFlash}
              disabled={
                compiling || flashing || !selectedPort || !deviceName.trim()
              }
              className="w-full flex items-center justify-center gap-2 px-4 py-3 bg-trellis-500 hover:bg-trellis-600 disabled:bg-zinc-800 disabled:text-zinc-600 text-white rounded-xl text-sm font-medium transition-colors disabled:cursor-not-allowed"
            >
              {compiling ? (
                <>
                  <Loader2 size={16} className="animate-spin" />
                  Compiling... {elapsedTime > 0 && `(${elapsedTime}s)`}
                </>
              ) : flashing ? (
                <>
                  <Loader2 size={16} className="animate-spin" />
                  Flashing... {elapsedTime > 0 && `(${elapsedTime}s)`}
                </>
              ) : (
                <>
                  <Hammer size={16} />
                  Compile & Flash
                </>
              )}
            </button>

            {!selectedPort && serialPorts.length === 0 && (
              <div className="flex items-center gap-2 p-3 bg-amber-500/5 border border-amber-500/20 rounded-lg">
                <AlertTriangle size={14} className="text-amber-400 flex-shrink-0" />
                <p className="text-xs text-amber-400/80">
                  No USB serial ports detected. Plug in your board and click
                  the refresh button.
                </p>
              </div>
            )}

            {flashSuccess && (
              <div className="flex items-center gap-2 p-3 bg-emerald-500/10 border border-emerald-500/20 rounded-lg">
                <Check size={14} className="text-emerald-400 flex-shrink-0" />
                <p className="text-xs text-emerald-400">
                  Flash successful! Your device should appear on the dashboard
                  shortly.
                </p>
              </div>
            )}

            {buildError && (
              <div className="flex items-center gap-2 p-3 bg-red-500/10 border border-red-500/20 rounded-lg">
                <AlertTriangle size={14} className="text-red-400 flex-shrink-0" />
                <p className="flex-1 text-xs text-red-400/80">
                  Build failed. Check the output below for details.
                </p>
                <button
                  onClick={handleCompileAndFlash}
                  className="flex items-center gap-1 px-2.5 py-1 bg-red-500/20 hover:bg-red-500/30 text-red-400 rounded-lg text-xs transition-colors flex-shrink-0"
                >
                  <RefreshCw size={11} />
                  Retry
                </button>
              </div>
            )}

            {buildOutput && (
              <div
                ref={outputRef}
                className="max-h-64 overflow-auto bg-zinc-950 border border-zinc-800 rounded-lg p-3 font-mono text-xs leading-relaxed select-text"
              >
                <pre
                  className={`whitespace-pre-wrap break-words ${
                    buildError
                      ? "text-red-400"
                      : flashSuccess
                        ? "text-trellis-400"
                        : "text-zinc-400"
                  }`}
                >
                  {buildOutput}
                </pre>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  };

  const renderDone = () => (
    <div className="max-w-md mx-auto text-center">
      {newDevice ? (
        <>
          <div className="w-16 h-16 bg-emerald-500/15 rounded-2xl flex items-center justify-center mx-auto mb-6">
            <Check size={32} className="text-emerald-400" />
          </div>
          <h2 className="text-xl font-bold text-zinc-100 mb-2">
            Device found!
          </h2>
          <p className="text-sm text-zinc-400 mb-2">
            <span className="text-zinc-200 font-medium">{newDevice.name}</span>{" "}
            appeared at{" "}
            <span className="font-mono text-xs text-zinc-300">
              {newDevice.ip}:{newDevice.port}
            </span>
          </p>
          {newDevice.capabilities.length > 0 && (
            <div className="flex flex-wrap justify-center gap-1.5 mb-6">
              {newDevice.capabilities.map((c) => (
                <span
                  key={c.id}
                  className="inline-flex items-center gap-1 px-2 py-0.5 bg-zinc-800 rounded-full text-[11px] text-zinc-400"
                >
                  {c.label}
                </span>
              ))}
            </div>
          )}
          <div className="flex gap-3 justify-center">
            <button
              onClick={finishOnboarding}
              className="flex items-center gap-2 px-5 py-2.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm font-medium transition-colors"
            >
              <Rocket size={14} />
              Open Dashboard
            </button>
            <button
              onClick={async () => {
                await markOnboardingDone();
                navigate(`/device/${encodeURIComponent(newDevice.id)}`);
              }}
              className="flex items-center gap-2 px-5 py-2.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
            >
              View Device
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="w-16 h-16 bg-zinc-800 rounded-2xl flex items-center justify-center mx-auto mb-6">
            <Radar size={32} className="text-zinc-500 animate-pulse" />
          </div>
          <h2 className="text-lg font-bold text-zinc-100 mb-2">
            Waiting for your device...
          </h2>
          <p className="text-sm text-zinc-400 mb-2">
            After flashing, your device will create a WiFi hotspot named{" "}
            <span className="font-mono text-xs text-trellis-400">
              Trellis-{deviceName.replace(/ /g, "-")}
            </span>
            .
          </p>
          <p className="text-sm text-zinc-500 mb-6">
            Connect to it from your phone, enter your WiFi credentials, and the
            device will join your network and appear here automatically.
          </p>
          <div className="flex gap-3 justify-center">
            <button
              onClick={finishOnboarding}
              className="flex items-center gap-2 px-5 py-2.5 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
            >
              Skip to Dashboard
            </button>
          </div>
        </>
      )}
    </div>
  );

  const stepContent = [
    renderWelcome,
    renderTemplateSelection,
    renderConfigureAndFlash,
    renderDone,
  ];

  const canAdvance =
    step === 0
      ? prereqsReady
      : step === 1
        ? selectedTemplate !== null
        : step === 2
          ? flashSuccess
          : true;

  return (
    <div className="flex flex-col h-full">
      {/* Step indicator */}
      <div className="flex items-center justify-center gap-2 mb-8 pt-2">
        {STEPS.map((label, i) => (
          <div key={label} className="flex items-center gap-2">
            <div className="flex items-center gap-1.5">
              <div
                className={`w-6 h-6 rounded-full flex items-center justify-center text-xs font-medium ${
                  i < step
                    ? "bg-trellis-500 text-white"
                    : i === step
                      ? "bg-trellis-500/20 text-trellis-400 ring-1 ring-trellis-500/40"
                      : "bg-zinc-800 text-zinc-600"
                }`}
              >
                {i < step ? <Check size={12} /> : i + 1}
              </div>
              <span
                className={`text-xs ${
                  i <= step ? "text-zinc-300" : "text-zinc-600"
                }`}
              >
                {label}
              </span>
            </div>
            {i < STEPS.length - 1 && (
              <div
                className={`w-8 h-px ${
                  i < step ? "bg-trellis-500/40" : "bg-zinc-800"
                }`}
              />
            )}
          </div>
        ))}
      </div>

      {/* Step content */}
      <div className="flex-1 overflow-auto px-4">{stepContent[step]()}</div>

      {/* Navigation */}
      <div className="flex items-center justify-between pt-4 mt-4 border-t border-zinc-800/50 px-4 pb-2">
        <button
          onClick={() => setStep(Math.max(0, step - 1))}
          disabled={step === 0}
          className="flex items-center gap-1.5 px-4 py-2 text-sm text-zinc-400 hover:text-zinc-200 disabled:opacity-0 transition-all"
        >
          <ChevronLeft size={14} />
          Back
        </button>

        {step < STEPS.length - 1 && (
          <button
            onClick={() => {
              if (step === 2) {
                setDeviceSnapshot(new Set(devices.map((d) => d.id)));
              }
              setStep(step + 1);
            }}
            disabled={!canAdvance}
            className="flex items-center gap-1.5 px-5 py-2 bg-trellis-500 hover:bg-trellis-600 disabled:bg-zinc-800 disabled:text-zinc-600 text-white rounded-lg text-sm font-medium transition-colors disabled:cursor-not-allowed"
          >
            {step === 2 ? "Find My Device" : "Next"}
            <ChevronRight size={14} />
          </button>
        )}
      </div>
    </div>
  );
}
