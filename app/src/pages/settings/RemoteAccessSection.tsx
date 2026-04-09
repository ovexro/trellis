import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Globe, ExternalLink, AlertTriangle, Check } from "lucide-react";
import type { RemoteProbeResult } from "./types";

type Props = {
  apiTokenCount: number;
};

export default function RemoteAccessSection({ apiTokenCount }: Props) {
  const [probeUrl, setProbeUrl] = useState("");
  const [probeToken, setProbeToken] = useState("");
  const [probeBusy, setProbeBusy] = useState(false);
  const [probeResult, setProbeResult] = useState<RemoteProbeResult | null>(null);

  useEffect(() => {
    invoke<string | null>("get_setting", { key: "remote_access_url" }).then((val) => {
      if (val) setProbeUrl(val);
    }).catch(() => {});
  }, []);

  const runReachabilityProbe = async () => {
    setProbeBusy(true);
    setProbeResult(null);
    try {
      const result = await invoke<RemoteProbeResult>("probe_remote_url", {
        url: probeUrl.trim(),
        token: probeToken.trim(),
      });
      setProbeResult(result);
      try {
        await invoke("set_setting", { key: "remote_access_url", value: probeUrl.trim() });
      } catch {
        // Non-fatal — the probe ran, the convenience save failed
      }
    } catch (err) {
      setProbeResult({
        ok: false,
        status: 0,
        latency_ms: 0,
        category: "validation_error",
        message: String(err),
      });
    } finally {
      setProbeBusy(false);
    }
  };

  return (
    <div>
      <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
        Remote Access
      </h2>
      <div className="space-y-3">
        <div className="flex items-start gap-2 text-sm text-zinc-300">
          <Globe size={16} className="text-trellis-400 mt-0.5 shrink-0" />
          <p>
            Reach Trellis from outside your home by exposing port <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-xs">9090</code> through a tunnel.
            The v0.3.4 token gate enforces auth at the destination, so the tunnel
            only forwards bytes — your secrets never leave this machine.
          </p>
        </div>

        {/* Safety check: warn loudly if zero tokens minted */}
        {apiTokenCount === 0 && (
          <div className="p-3 bg-amber-500/10 border border-amber-500/30 rounded-lg">
            <div className="flex items-start gap-2">
              <AlertTriangle size={16} className="text-amber-400 mt-0.5 shrink-0" />
              <div className="text-sm">
                <p className="text-amber-300 font-medium">Mint an API token first</p>
                <p className="text-amber-300/70 text-xs mt-1">
                  You haven't created any API tokens yet. Without one, every non-loopback
                  request hits a 401 — the tunnel will be reachable but completely unusable.
                  Scroll up to <strong>API Tokens → Create token</strong>.
                </p>
              </div>
            </div>
          </div>
        )}

        {/* Cloudflare Tunnel — primary recommendation */}
        <div className="border border-zinc-800 rounded-lg overflow-hidden">
          <div className="bg-zinc-900/40 px-4 py-2.5 border-b border-zinc-800 flex items-center gap-2">
            <span className="text-sm font-medium text-zinc-200">Cloudflare Tunnel</span>
            <span className="text-[10px] uppercase tracking-wider text-trellis-400 bg-trellis-500/10 border border-trellis-500/30 rounded-full px-2 py-0.5">
              Recommended
            </span>
          </div>
          <div className="p-4 space-y-3 text-sm">
            <p className="text-zinc-400">
              Free, no inbound port, branded URL on your own domain.
              Composes with Cloudflare Access for free SSO if you want
              defense in depth on top of the token gate.
            </p>
            <ol className="text-zinc-300 text-xs space-y-1.5 list-decimal list-inside pl-1">
              <li>Add a domain to your Cloudflare account (free tier is fine)</li>
              <li>
                Install <code className="px-1 bg-zinc-800 rounded">cloudflared</code>:{" "}
                <a
                  href="https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-trellis-400 hover:text-trellis-300 inline-flex items-center gap-0.5"
                >
                  download
                  <ExternalLink size={11} />
                </a>
              </li>
              <li><code className="px-1 bg-zinc-800 rounded">cloudflared tunnel login</code></li>
              <li><code className="px-1 bg-zinc-800 rounded">cloudflared tunnel create trellis</code></li>
              <li>
                <code className="px-1 bg-zinc-800 rounded">cloudflared tunnel route dns trellis trellis.&lt;your-domain&gt;</code>
              </li>
              <li>
                <code className="px-1 bg-zinc-800 rounded">cloudflared tunnel run --url http://localhost:9090 trellis</code>
              </li>
            </ol>
            <p className="text-xs text-zinc-500">
              Then open <code className="px-1 bg-zinc-800 rounded">https://trellis.&lt;your-domain&gt;</code> on your phone.
              The dashboard's first <code className="px-1 bg-zinc-800 rounded">/api/*</code> call will return 401, an auth modal pops up,
              paste your token once, done. The browser remembers it in localStorage.
            </p>
          </div>
        </div>

        {/* Tailscale Funnel — secondary, no-domain alternative */}
        <div className="border border-zinc-800 rounded-lg overflow-hidden">
          <div className="bg-zinc-900/40 px-4 py-2.5 border-b border-zinc-800 flex items-center gap-2">
            <span className="text-sm font-medium text-zinc-200">Tailscale Funnel</span>
            <span className="text-[10px] uppercase tracking-wider text-zinc-400 bg-zinc-700/30 border border-zinc-700 rounded-full px-2 py-0.5">
              No domain needed
            </span>
          </div>
          <div className="p-4 space-y-3 text-sm">
            <p className="text-zinc-400">
              Three commands, no DNS, no domain. URL is <code className="px-1 bg-zinc-800 rounded">*.ts.net</code>.
              Personal use is free up to 100 devices.
            </p>
            <ol className="text-zinc-300 text-xs space-y-1.5 list-decimal list-inside pl-1">
              <li>
                <a
                  href="https://tailscale.com/kb/1031/install-linux"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-trellis-400 hover:text-trellis-300 inline-flex items-center gap-0.5"
                >
                  Install Tailscale
                  <ExternalLink size={11} />
                </a>
              </li>
              <li><code className="px-1 bg-zinc-800 rounded">sudo tailscale up</code></li>
              <li><code className="px-1 bg-zinc-800 rounded">sudo tailscale funnel 9090 on</code></li>
            </ol>
            <p className="text-xs text-zinc-500">
              Tailscale prints your funnel URL. Open it on your phone and paste your token at the auth prompt.
            </p>
          </div>
        </div>

        {/* Reachability probe widget */}
        <div className="border border-zinc-800 rounded-lg p-4 space-y-3">
          <div>
            <p className="text-sm font-medium text-zinc-200">Test reachability</p>
            <p className="text-xs text-zinc-500 mt-0.5">
              Verify your tunnel + token combo end-to-end before pulling out your phone.
              This sends a single GET to <code className="px-1 bg-zinc-800 rounded">&lt;url&gt;/api/devices</code> from
              this machine and reports what happened. The token is held in
              memory only — never persisted.
            </p>
          </div>
          <input
            type="text"
            value={probeUrl}
            onChange={(e) => setProbeUrl(e.target.value)}
            placeholder="https://trellis.example.com"
            className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-trellis-500 font-mono"
            disabled={probeBusy}
          />
          <input
            type="password"
            value={probeToken}
            onChange={(e) => setProbeToken(e.target.value)}
            placeholder="trls_..."
            autoComplete="off"
            className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-trellis-500 font-mono"
            disabled={probeBusy}
            onKeyDown={(e) => { if (e.key === "Enter") runReachabilityProbe(); }}
          />
          <div className="flex items-center gap-3">
            <button
              onClick={runReachabilityProbe}
              disabled={probeBusy || !probeUrl.trim() || !probeToken.trim()}
              className="px-4 py-2 bg-trellis-600 hover:bg-trellis-500 disabled:bg-zinc-700 disabled:opacity-50 text-white rounded-lg text-sm transition-colors"
            >
              {probeBusy ? "Probing…" : "Test reachability"}
            </button>
            {probeResult && (
              <span className="text-xs text-zinc-500">
                HTTP {probeResult.status || "—"} · {probeResult.latency_ms} ms
              </span>
            )}
          </div>
          {probeResult && (
            <div
              className={`p-3 rounded-lg border text-sm ${
                probeResult.category === "success"
                  ? "bg-trellis-500/10 border-trellis-500/30 text-trellis-300"
                  : "bg-amber-500/10 border-amber-500/30 text-amber-300"
              }`}
            >
              <div className="flex items-start gap-2">
                {probeResult.category === "success" ? (
                  <Check size={14} className="mt-0.5 shrink-0" />
                ) : (
                  <AlertTriangle size={14} className="mt-0.5 shrink-0" />
                )}
                <span>{probeResult.message}</span>
              </div>
            </div>
          )}
        </div>

        <p className="text-xs text-zinc-600">
          Why not ngrok? The free tier rotates your URL every restart, which doesn't work for
          "set it once, my phone uses this URL all year". Paid stable URLs are $8+/mo per user —
          both options above are strictly better and free.
        </p>
      </div>
    </div>
  );
}
