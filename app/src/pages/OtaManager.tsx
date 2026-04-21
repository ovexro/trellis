import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { Upload, CheckCircle, AlertCircle, FileUp, History, RotateCcw, Trash2, HardDriveDownload, Loader2, Github, RefreshCw, Download, XCircle } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

interface GithubAsset {
  name: string;
  size: number;
  download_url: string;
}

interface GithubRelease {
  tag: string;
  name: string;
  published_at: string;
  prerelease: boolean;
  assets: GithubAsset[];
}

// How long to wait after the desktop has flushed the firmware bytes before
// we give up watching for the device's uptime to reset. The device has to:
// finish writing flash (~10s), reboot (~3s), reconnect WiFi + WS (~5s),
// and emit a heartbeat (~10s window). 60s is generous-but-not-forever.
const REBOOT_WATCH_MS = 60_000;

interface FirmwareRecord {
  id: number;
  device_id: string;
  version: string;
  file_path: string;
  file_size: number;
  uploaded_at: string;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function OtaManager() {
  const { devices } = useDeviceStore();
  const [selectedDevice, setSelectedDevice] = useState("");
  const [firmwarePath, setFirmwarePath] = useState("");
  const [otaProgress, setOtaProgress] = useState(-1);
  const [status, setStatus] = useState<"idle" | "uploading" | "delivered" | "success" | "error" | "cancelled">("idle");
  const [errorMsg, setErrorMsg] = useState("");
  // Set by the v0.16.0 `ota_applied` event — the device POSTed an apply
  // confirmation to /api/ota/ack/<nonce>, proving the new firmware not
  // only transferred but booted. Cleared when the user selects a
  // different device or starts a new upload.
  const [applyConfirmed, setApplyConfirmed] = useState(false);
  const [firmwareHistory, setFirmwareHistory] = useState<FirmwareRecord[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [dragging, setDragging] = useState(false);

  // GitHub OTA state
  const [ghRepo, setGhRepo] = useState("");
  const [ghReleases, setGhReleases] = useState<GithubRelease[]>([]);
  const [ghChecking, setGhChecking] = useState(false);
  const [ghError, setGhError] = useState("");
  const [ghShowPrerelease, setGhShowPrerelease] = useState(false);
  const [ghAssetFilter, setGhAssetFilter] = useState("");
  const [ghDownloading, setGhDownloading] = useState<string | null>(null);
  const [ghDownloadPct, setGhDownloadPct] = useState(-1);
  const [ghDownloadTotal, setGhDownloadTotal] = useState(0);

  // Tracks the in-flight OTA so events from a different selected device
  // don't get mis-routed and so the reboot watcher knows what uptime
  // baseline to compare against. Cleared on success/error/idle.
  const inFlightRef = useRef<{ deviceId: string; uptimeBaseline: number } | null>(null);
  const rebootTimerRef = useRef<number | null>(null);

  const onlineDevices = devices.filter((d) => d.online);
  const selectedDeviceObj = devices.find((d) => d.id === selectedDevice);

  // Listen for Tauri drag-drop events (provides file paths)
  useEffect(() => {
    const unlistenDrop = listen<{ paths: string[] }>("tauri://drag-drop", (e) => {
      setDragging(false);
      const paths = e.payload.paths;
      if (paths && paths.length > 0) {
        const path = paths[0];
        if (path.endsWith(".bin")) {
          setFirmwarePath(path);
          setStatus("idle");
        }
      }
    });
    const unlistenEnter = listen("tauri://drag-enter", () => setDragging(true));
    const unlistenLeave = listen("tauri://drag-leave", () => setDragging(false));
    return () => {
      unlistenDrop.then((fn) => fn());
      unlistenEnter.then((fn) => fn());
      unlistenLeave.then((fn) => fn());
    };
  }, []);

  // GitHub download progress (desktop → GitHub, before device OTA begins)
  useEffect(() => {
    const unlisten = listen<{
      device_id: string;
      downloaded: number;
      total: number;
      percent: number;
    }>("gh-download-progress", (e) => {
      const inFlight = inFlightRef.current;
      if (!inFlight || e.payload.device_id !== inFlight.deviceId) return;
      setGhDownloadPct(e.payload.percent);
      setGhDownloadTotal(e.payload.total);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<{
      device_id: string;
      event_type: string;
      payload: { percent?: number; bytes?: number; error?: string };
    }>(
      "device-event",
      (e) => {
        const inFlight = inFlightRef.current;
        if (!inFlight || e.payload.device_id !== inFlight.deviceId) return;

        const { event_type, payload } = e.payload;

        if (event_type === "ota_progress") {
          // The library streams real progress (every 5%) during OTA
          // download via the httpUpdate.onProgress callback. If the WS
          // stays up, these arrive as 0, 5, 10, ..., 100. Fallback to
          // the reboot watcher still works if the WS drops mid-transfer.
          const pct = payload.percent ?? -1;
          setOtaProgress(pct);
          if (pct === -1) {
            setStatus("error");
            setErrorMsg("Device reported OTA failure.");
            inFlightRef.current = null;
          } else if (pct === 100) {
            setStatus("success");
            inFlightRef.current = null;
          }
        } else if (event_type === "ota_delivered") {
          // Fired twice: once from the desktop HTTP server (bytes flushed)
          // and once from the device itself (firmware written to flash,
          // about to reboot). Both are safe to handle identically.
          setStatus("delivered");
          setOtaProgress(0);
        } else if (event_type === "ota_delivery_failed") {
          // User-initiated cancels surface as ota_delivery_failed with
          // error="cancelled" — route those to a distinct "cancelled"
          // state so the UI doesn't show an alarming "OTA update failed"
          // banner for the user's own abort.
          if (payload.error === "cancelled") {
            setStatus("cancelled");
            setErrorMsg("");
          } else {
            setStatus("error");
            setErrorMsg(payload.error ? `Delivery failed: ${payload.error}` : "Delivery failed.");
          }
          inFlightRef.current = null;
        }
      },
    );
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // `ota_applied` (v0.16.0) arrives AFTER the reboot watcher has cleared
  // inFlightRef, so the in-flight-gated listener above would drop it. A
  // dedicated listener keyed off selectedDevice handles it instead. Uses
  // a ref for selectedDevice so the effect can stay mounted for the
  // component's lifetime without re-subscribing on every selection change.
  const selectedDeviceRef = useRef(selectedDevice);
  useEffect(() => {
    selectedDeviceRef.current = selectedDevice;
  }, [selectedDevice]);
  useEffect(() => {
    const unlisten = listen<{ device_id: string; event_type: string }>(
      "device-event",
      (e) => {
        if (
          e.payload.event_type === "ota_applied" &&
          e.payload.device_id === selectedDeviceRef.current
        ) {
          setApplyConfirmed(true);
        }
      },
    );
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // Clear the apply-confirmed chip when the user navigates to a different
  // device (its own OTA status is unrelated) or kicks off a fresh upload.
  useEffect(() => {
    setApplyConfirmed(false);
  }, [selectedDevice]);

  // Reboot watcher: once delivery completes, watch the in-flight device's
  // uptime in the store. The device's WebSocket reconnects within ~5s of
  // boot and the next heartbeat (within 10s) carries a fresh uptime that
  // is lower than the baseline we captured at click time. That's our
  // "reboot confirmed" signal.
  useEffect(() => {
    if (status !== "delivered") return;
    const inFlight = inFlightRef.current;
    if (!inFlight) return;
    const dev = devices.find((d) => d.id === inFlight.deviceId);
    if (!dev || !dev.online) return;
    if (dev.system.uptime_s < inFlight.uptimeBaseline) {
      setStatus("success");
      inFlightRef.current = null;
      if (rebootTimerRef.current != null) {
        window.clearTimeout(rebootTimerRef.current);
        rebootTimerRef.current = null;
      }
    }
  }, [devices, status]);

  // Reboot watch timeout: if we don't see the uptime drop within
  // REBOOT_WATCH_MS, fall through to a soft-success state. The device
  // most likely rebooted but is on a different LAN segment / mDNS hasn't
  // re-discovered it / the heartbeat hasn't fired yet.
  useEffect(() => {
    if (status !== "delivered") {
      if (rebootTimerRef.current != null) {
        window.clearTimeout(rebootTimerRef.current);
        rebootTimerRef.current = null;
      }
      return;
    }
    if (rebootTimerRef.current != null) return;
    rebootTimerRef.current = window.setTimeout(() => {
      // Don't mark error — the OTA bytes were delivered. The device
      // probably rebooted; we just couldn't confirm via heartbeat.
      setStatus("success");
      inFlightRef.current = null;
      rebootTimerRef.current = null;
    }, REBOOT_WATCH_MS);
    return () => {
      if (rebootTimerRef.current != null) {
        window.clearTimeout(rebootTimerRef.current);
        rebootTimerRef.current = null;
      }
    };
  }, [status]);

  const loadFirmwareHistory = async (deviceId: string) => {
    if (!deviceId) {
      setFirmwareHistory([]);
      return;
    }
    setHistoryLoading(true);
    try {
      const history = await invoke<FirmwareRecord[]>("get_firmware_history", { deviceId });
      setFirmwareHistory(history);
    } catch {
      setFirmwareHistory([]);
    } finally {
      setHistoryLoading(false);
    }
  };

  useEffect(() => {
    loadFirmwareHistory(selectedDevice);
    // Restore per-device GitHub repo + asset filter bindings
    if (selectedDevice) {
      invoke<{ value: string }>("get_setting", { key: `github_ota_${selectedDevice}` })
        .then((r) => { if (r?.value) setGhRepo(r.value); else setGhRepo(""); })
        .catch(() => setGhRepo(""));
      invoke<{ value: string }>("get_setting", { key: `github_ota_filter_${selectedDevice}` })
        .then((r) => { if (r?.value) setGhAssetFilter(r.value); else setGhAssetFilter(""); })
        .catch(() => setGhAssetFilter(""));
    } else {
      setGhRepo("");
      setGhAssetFilter("");
    }
    setGhReleases([]);
    setGhError("");
  }, [selectedDevice]);

  const handleCheckGithub = async () => {
    const trimmed = ghRepo.trim();
    if (!trimmed) return;
    const parts = trimmed.replace(/^https?:\/\/github\.com\//, "").replace(/\/$/, "").split("/");
    if (parts.length < 2) {
      setGhError("Enter a repo as owner/repo (e.g. ovexro/trellis)");
      return;
    }
    const [owner, repo] = parts;
    setGhChecking(true);
    setGhError("");
    setGhReleases([]);
    try {
      const releases = await invoke<GithubRelease[]>("check_github_releases", { owner, repo });
      setGhReleases(releases);
      if (releases.length === 0) {
        setGhError("No releases with .bin or .bin.gz firmware assets found. This feature works with repos that publish compiled ESP32 firmware in their GitHub Releases.");
      }
      // Save repo binding for this device
      if (selectedDevice) {
        invoke("set_setting", { key: `github_ota_${selectedDevice}`, value: trimmed }).catch(() => {});
      }
    } catch (err) {
      setGhError(String(err));
    } finally {
      setGhChecking(false);
    }
  };

  const handleGithubFlash = async (release: GithubRelease, asset: GithubAsset) => {
    const device = devices.find((d) => d.id === selectedDevice);
    if (!device) return;
    setGhDownloading(asset.download_url);
    setStatus("uploading");
    setErrorMsg("");
    setApplyConfirmed(false);
    setOtaProgress(-1);
    setGhDownloadPct(0);
    setGhDownloadTotal(0);
    inFlightRef.current = {
      deviceId: device.id,
      uptimeBaseline: device.system.uptime_s,
    };
    try {
      await invoke("start_github_ota", {
        deviceId: device.id,
        ip: device.ip,
        port: device.port,
        downloadUrl: asset.download_url,
        releaseTag: release.tag,
        assetName: asset.name,
      });
      loadFirmwareHistory(selectedDevice);
    } catch (err) {
      setStatus("error");
      setErrorMsg(String(err));
      inFlightRef.current = null;
    } finally {
      setGhDownloading(null);
      setGhDownloadPct(-1);
      setGhDownloadTotal(0);
    }
  };

  const handleRollback = async (record: FirmwareRecord) => {
    const device = devices.find((d) => d.id === selectedDevice);
    if (!device) return;
    setStatus("uploading");
    setErrorMsg("");
    setApplyConfirmed(false);
    setOtaProgress(0);
    inFlightRef.current = {
      deviceId: device.id,
      uptimeBaseline: device.system.uptime_s,
    };
    try {
      await invoke("rollback_firmware", {
        deviceId: device.id,
        ip: device.ip,
        port: device.port,
        firmwareRecordPath: record.file_path,
      });
    } catch (err) {
      setStatus("error");
      setErrorMsg(String(err));
      inFlightRef.current = null;
    }
  };

  // Aborts an in-flight OTA by flipping the stop_flag that serve_firmware
  // registered on start. The desktop HTTP server breaks out of its chunked
  // write loop within ~500ms, persists status="cancelled" on the matching
  // firmware_history row, and emits ota_delivery_failed with error
  // "cancelled" — the same event listener below flips the UI to its
  // error state. No UI changes needed here beyond the button itself.
  const handleCancel = async () => {
    const inFlight = inFlightRef.current;
    if (!inFlight) return;
    try {
      await invoke<boolean>("cancel_ota", { deviceId: inFlight.deviceId });
    } catch (err) {
      setErrorMsg(`Failed to send cancel: ${err}`);
    }
  };

  const handleDeleteRecord = async (record: FirmwareRecord) => {
    if (!confirm(`Delete stored firmware v${record.version} (${formatFileSize(record.file_size)})?`)) return;
    try {
      await invoke("delete_firmware_record", { id: record.id });
      setFirmwareHistory((prev) => prev.filter((r) => r.id !== record.id));
    } catch (err) {
      setErrorMsg(String(err));
    }
  };

  const handleUpload = async () => {
    if (!selectedDevice || !firmwarePath) return;

    const device = devices.find((d) => d.id === selectedDevice);
    if (!device) return;

    setStatus("uploading");
    setErrorMsg("");
    setApplyConfirmed(false);
    setOtaProgress(0);
    // Capture the uptime baseline NOW, while the device is still happily
    // running and heartbeats are landing. The reboot watcher uses this
    // to detect "uptime dropped → device rebooted → OTA succeeded".
    inFlightRef.current = {
      deviceId: device.id,
      uptimeBaseline: device.system.uptime_s,
    };

    try {
      await invoke("start_ota", {
        deviceId: device.id,
        ip: device.ip,
        port: device.port,
        firmwarePath: firmwarePath,
      });
      // Don't set success here — the OTA command was SENT but the device
      // hasn't finished downloading yet. Status will update via the
      // ota_delivered event (from the desktop's serve_firmware) and then
      // the reboot watcher.
      loadFirmwareHistory(selectedDevice);
    } catch (err) {
      setStatus("error");
      setErrorMsg(String(err));
      inFlightRef.current = null;
    }
  };

  return (
    <div>
      <h1 className="text-xl font-bold text-zinc-100 mb-2">
        Firmware Update (OTA)
      </h1>
      <p className="text-sm text-zinc-500 mb-6">
        Upload a local .bin file or pull firmware directly from a GitHub release.
        The device will download the firmware from your PC and reboot.
      </p>

      <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-6 space-y-5">
        <div>
          <label className="block text-sm font-medium text-zinc-300 mb-2">
            Target Device
          </label>
          <select
            value={selectedDevice}
            onChange={(e) => {
              setSelectedDevice(e.target.value);
              setStatus("idle");
            }}
            className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
          >
            <option value="">Select a device...</option>
            {onlineDevices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name} ({d.ip}) — FW {d.firmware} — {d.platform}
              </option>
            ))}
          </select>
          {onlineDevices.length === 0 && (
            <p className="text-xs text-zinc-600 mt-1">
              No online devices found. Connect a device first.
            </p>
          )}
        </div>

        <div>
          <label className="block text-sm font-medium text-zinc-300 mb-2">
            Firmware File (.bin)
          </label>
          <div
            className={`border-2 border-dashed rounded-xl p-4 transition-colors ${
              dragging
                ? "border-trellis-500 bg-trellis-500/5"
                : firmwarePath
                  ? "border-zinc-700 bg-zinc-800/30"
                  : "border-zinc-700/50 bg-zinc-800/20"
            }`}
          >
            {firmwarePath ? (
              <div className="flex items-center gap-3">
                <HardDriveDownload size={20} className="text-trellis-400 flex-shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-zinc-300 font-mono truncate">{firmwarePath}</p>
                  <p className="text-xs text-zinc-600 mt-0.5">Ready to upload</p>
                </div>
                <button
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 rounded-lg text-xs transition-colors"
                  onClick={async () => {
                    const file = await open({
                      multiple: false,
                      filters: [{ name: "Firmware", extensions: ["bin"] }],
                    });
                    if (file) {
                      setFirmwarePath(file);
                      setStatus("idle");
                    }
                  }}
                >
                  <FileUp size={12} />
                  Change
                </button>
              </div>
            ) : (
              <div className="text-center py-3">
                <FileUp size={24} className={`mx-auto mb-2 ${dragging ? "text-trellis-400" : "text-zinc-600"}`} />
                <p className="text-sm text-zinc-400">
                  {dragging ? "Drop .bin file here" : "Drag & drop a .bin file here"}
                </p>
                <p className="text-xs text-zinc-600 mt-1 mb-3">or</p>
                <button
                  className="flex items-center gap-1.5 px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors mx-auto"
                  onClick={async () => {
                    const file = await open({
                      multiple: false,
                      filters: [{ name: "Firmware", extensions: ["bin"] }],
                    });
                    if (file) {
                      setFirmwarePath(file);
                      setStatus("idle");
                    }
                  }}
                >
                  <FileUp size={14} />
                  Browse for file
                </button>
              </div>
            )}
          </div>
        </div>

        {selectedDeviceObj && selectedDeviceObj.platform !== "esp32" && (
          <div className="flex items-center gap-2 text-sm text-amber-400 bg-amber-500/10 p-3 rounded-lg">
            <AlertCircle size={16} />
            OTA is currently supported on ESP32 only. Pico W support is planned.
          </div>
        )}

        <button
          onClick={handleUpload}
          disabled={
            !selectedDevice ||
            !firmwarePath ||
            status === "uploading" ||
            status === "delivered" ||
            (selectedDeviceObj?.platform !== "esp32")
          }
          className="flex items-center gap-2 px-4 py-2.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50 w-full justify-center"
        >
          <Upload size={16} />
          {status === "uploading"
            ? "Sending to device..."
            : status === "delivered"
              ? "Waiting for reboot..."
              : "Upload Firmware"}
        </button>

        {ghDownloading && ghDownloadPct >= 0 && (
          <div>
            <div className="flex justify-between text-xs text-zinc-400 mb-1">
              <span>Downloading from GitHub…</span>
              <span>{ghDownloadPct}%{ghDownloadTotal > 0 ? ` · ${formatFileSize(ghDownloadTotal)}` : ""}</span>
            </div>
            <div className="w-full h-2 bg-zinc-800 rounded-full overflow-hidden">
              <div
                className="h-full bg-trellis-500 rounded-full transition-all duration-300"
                style={{ width: `${ghDownloadPct}%` }}
              />
            </div>
          </div>
        )}

        {status === "uploading" && !ghDownloading && otaProgress >= 0 && (
          <div>
            <div className="flex justify-between text-xs text-zinc-400 mb-1">
              <span>Downloading firmware to device...</span>
              <span>{otaProgress}%</span>
            </div>
            <div className="w-full h-2 bg-zinc-800 rounded-full overflow-hidden">
              <div
                className="h-full bg-trellis-500 rounded-full transition-all duration-300"
                style={{ width: `${otaProgress}%` }}
              />
            </div>
          </div>
        )}

        {status === "uploading" && (
          <button
            onClick={handleCancel}
            className="flex items-center gap-2 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-xs transition-colors w-full justify-center"
            title="Abort this OTA transfer"
          >
            <XCircle size={14} />
            Cancel transfer
          </button>
        )}

        {status === "delivered" && (
          <div className="flex items-start gap-2 text-sm text-trellis-400 bg-trellis-500/10 p-3 rounded-lg">
            <Loader2 size={16} className="mt-0.5 flex-shrink-0 animate-spin" />
            <div>
              <p>Firmware delivered. Waiting for device to reboot…</p>
              <p className="text-xs mt-1 text-trellis-300/70">
                The device&rsquo;s WebSocket drops during OTA flashing,
                so we&rsquo;re watching for its uptime counter to reset
                via the next heartbeat.
              </p>
            </div>
          </div>
        )}

        {status === "success" && (
          <div className="flex items-start gap-2 text-sm text-trellis-400 bg-trellis-500/10 p-3 rounded-lg">
            <CheckCircle size={16} className="mt-0.5 flex-shrink-0" />
            <div>
              <div className="flex items-center gap-2 flex-wrap">
                <span>Firmware update complete. The device has rebooted.</span>
                {applyConfirmed && (
                  <span className="inline-flex items-center gap-1 text-[10px] font-medium uppercase tracking-wider px-1.5 py-0.5 rounded bg-trellis-500/20 text-trellis-300 border border-trellis-500/30">
                    Apply confirmed
                  </span>
                )}
              </div>
              {applyConfirmed && (
                <p className="text-xs mt-1 text-trellis-300/70">
                  The device POSTed an apply confirmation — the new firmware
                  is running, not just written.
                </p>
              )}
            </div>
          </div>
        )}

        {status === "error" && (
          <div className="flex items-start gap-2 text-sm text-red-400 bg-red-500/10 p-3 rounded-lg">
            <AlertCircle size={16} className="mt-0.5 flex-shrink-0" />
            <div>
              <p>OTA update failed.</p>
              {errorMsg && <p className="text-xs mt-1 text-red-300">{errorMsg}</p>}
            </div>
          </div>
        )}

        {status === "cancelled" && (
          <div className="flex items-start gap-2 text-sm text-zinc-400 bg-zinc-800/50 p-3 rounded-lg">
            <XCircle size={16} className="mt-0.5 flex-shrink-0" />
            <div>
              <p>OTA transfer cancelled.</p>
              <p className="text-xs mt-1 text-zinc-500">
                The device did not receive a complete firmware image and
                will continue running its current version.
              </p>
            </div>
          </div>
        )}
      </div>

      {/* GitHub Release OTA */}
      {selectedDevice && selectedDeviceObj?.platform === "esp32" && (
        <div className="mt-6 bg-zinc-900 border border-zinc-800 rounded-xl p-6">
          <div className="flex items-center gap-2 mb-4">
            <Github size={16} className="text-zinc-400" />
            <h2 className="text-sm font-semibold text-zinc-200">Update from GitHub Release</h2>
          </div>
          <p className="text-xs text-zinc-500 mb-4">
            Point to a GitHub repository that publishes .bin or .bin.gz firmware files in its releases.
            Compressed .bin.gz files are automatically decompressed before flashing.
          </p>

          <div className="flex gap-2 mb-4">
            <input
              type="text"
              value={ghRepo}
              onChange={(e) => setGhRepo(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") handleCheckGithub(); }}
              placeholder="owner/repo (e.g. ovexro/trellis)"
              className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder:text-zinc-600"
            />
            <button
              onClick={handleCheckGithub}
              disabled={ghChecking || !ghRepo.trim()}
              className="flex items-center gap-1.5 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors disabled:opacity-50"
            >
              {ghChecking ? <Loader2 size={14} className="animate-spin" /> : <RefreshCw size={14} />}
              Check
            </button>
          </div>

          {ghReleases.length > 0 && (
            <input
              type="text"
              value={ghAssetFilter}
              onChange={(e) => setGhAssetFilter(e.target.value)}
              onBlur={() => {
                if (selectedDevice) {
                  invoke("set_setting", { key: `github_ota_filter_${selectedDevice}`, value: ghAssetFilter.trim() }).catch(() => {});
                }
              }}
              placeholder="Filter assets by name (e.g. esp32, firmware-v2)"
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder:text-zinc-600 mb-4"
            />
          )}

          {ghError && (
            <div className="flex items-center gap-2 text-sm text-amber-400 bg-amber-500/10 p-3 rounded-lg mb-4">
              <AlertCircle size={16} className="flex-shrink-0" />
              {ghError}
            </div>
          )}

          {ghReleases.length > 0 && (() => {
            const hasPrerelease = ghReleases.some((r) => r.prerelease);
            const filterLc = ghAssetFilter.trim().toLowerCase();
            const matchesFilter = (a: GithubAsset) => !filterLc || a.name.toLowerCase().includes(filterLc);
            const filtered = (ghShowPrerelease ? ghReleases : ghReleases.filter((r) => !r.prerelease))
              .filter((r) => r.assets.some(matchesFilter));
            return (
            <div className="space-y-2">
              {hasPrerelease && (
                <label className="flex items-center gap-2 text-xs text-zinc-500 cursor-pointer select-none">
                  <input
                    type="checkbox"
                    checked={ghShowPrerelease}
                    onChange={(e) => setGhShowPrerelease(e.target.checked)}
                    className="accent-trellis-500"
                  />
                  Show pre-releases ({ghReleases.filter((r) => r.prerelease).length})
                </label>
              )}
              {filtered.length === 0 && (
                <p className="text-xs text-zinc-600">No stable releases with firmware assets. Enable "Show pre-releases" above.</p>
              )}
              {filtered.map((rel) => (
                <div key={rel.tag} className="bg-zinc-800/50 border border-zinc-700/50 rounded-lg p-4">
                  <div className="flex items-center justify-between mb-2">
                    <div>
                      <span className="text-sm font-mono text-zinc-200">{rel.tag}</span>
                      {rel.prerelease && (
                        <span className="text-[10px] text-amber-400 ml-2 uppercase tracking-wide">
                          pre-release
                        </span>
                      )}
                      {rel.name !== rel.tag && (
                        <span className="text-xs text-zinc-500 ml-2">{rel.name}</span>
                      )}
                      {selectedDeviceObj?.firmware && rel.tag.replace(/^v/, "") === selectedDeviceObj.firmware && (
                        <span className="text-[10px] text-trellis-400 ml-2 uppercase tracking-wide">
                          current
                        </span>
                      )}
                    </div>
                    <span className="text-xs text-zinc-600">
                      {new Date(rel.published_at).toLocaleDateString()}
                    </span>
                  </div>
                  <div className="space-y-1.5">
                    {rel.assets.filter((a) => !ghAssetFilter.trim() || a.name.toLowerCase().includes(ghAssetFilter.trim().toLowerCase())).map((asset) => (
                      <div key={asset.name} className="flex items-center justify-between">
                        <div className="flex items-center gap-2 min-w-0 flex-1">
                          <Download size={12} className="text-zinc-500 flex-shrink-0" />
                          <span className="text-xs text-zinc-400 font-mono truncate">{asset.name}</span>
                          <span className="text-xs text-zinc-600">{formatFileSize(asset.size)}</span>
                        </div>
                        <button
                          onClick={() => handleGithubFlash(rel, asset)}
                          disabled={
                            status === "uploading" ||
                            status === "delivered" ||
                            ghDownloading !== null ||
                            !selectedDeviceObj?.online
                          }
                          className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-trellis-500/10 text-trellis-400 hover:bg-trellis-500/20 rounded-md transition-colors disabled:opacity-40 ml-3 flex-shrink-0"
                        >
                          {ghDownloading === asset.download_url ? (
                            <Loader2 size={12} className="animate-spin" />
                          ) : (
                            <Upload size={12} />
                          )}
                          Flash
                        </button>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
            );
          })()}
        </div>
      )}

      {/* Firmware History */}
      {selectedDevice && (
        <div className="mt-6 bg-zinc-900 border border-zinc-800 rounded-xl p-6">
          <div className="flex items-center gap-2 mb-4">
            <History size={16} className="text-zinc-400" />
            <h2 className="text-sm font-semibold text-zinc-200">Firmware History</h2>
          </div>

          {selectedDeviceObj && selectedDeviceObj.firmware && (
            <div className="flex items-center gap-2 text-xs text-zinc-500 mb-4">
              <span>Current firmware:</span>
              <span className="text-zinc-300 font-mono px-2 py-0.5 bg-trellis-500/10 border border-trellis-500/20 rounded">
                v{selectedDeviceObj.firmware}
              </span>
            </div>
          )}

          {historyLoading ? (
            <p className="text-xs text-zinc-600">Loading history...</p>
          ) : firmwareHistory.length === 0 ? (
            <p className="text-xs text-zinc-600">No firmware history yet. Upload a firmware to start tracking.</p>
          ) : (
            <div className="space-y-2">
              {firmwareHistory.map((record) => {
                const isCurrent = selectedDeviceObj?.firmware === record.version;
                return (
                <div
                  key={record.id}
                  className={`flex items-center justify-between rounded-lg px-4 py-3 ${
                    isCurrent
                      ? "bg-trellis-500/5 border border-trellis-500/20"
                      : "bg-zinc-800/50 border border-zinc-700/50"
                  }`}
                >
                  <div className="min-w-0 flex-1">
                    <p className="text-sm text-zinc-300 font-mono truncate">
                      v{record.version}
                      {isCurrent && (
                        <span className="text-[10px] text-trellis-400 ml-2 font-sans uppercase tracking-wide">
                          current
                        </span>
                      )}
                    </p>
                    <p className="text-xs text-zinc-500 mt-0.5">
                      {formatFileSize(record.file_size)} &middot; {new Date(record.uploaded_at + "Z").toLocaleString()}
                    </p>
                  </div>
                  <div className="flex items-center gap-2 ml-4 flex-shrink-0">
                    <button
                      onClick={() => handleRollback(record)}
                      disabled={status === "uploading" || status === "delivered" || !selectedDeviceObj?.online}
                      title="Rollback to this firmware"
                      className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 rounded-md transition-colors disabled:opacity-40"
                    >
                      <RotateCcw size={12} />
                      Rollback
                    </button>
                    <button
                      onClick={() => handleDeleteRecord(record)}
                      title="Delete stored firmware"
                      className="flex items-center gap-1.5 px-2 py-1.5 text-xs text-zinc-500 hover:text-red-400 hover:bg-red-500/10 rounded-md transition-colors"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
