import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Key, Copy, Trash2, AlertTriangle, Check, Clock, Shield, Eye } from "lucide-react";
import type { ApiToken, CreatedApiToken } from "./types";
import { formatTimestamp } from "./types";

const TTL_OPTIONS = [
  { value: "never", label: "Never expires" },
  { value: "1h", label: "1 hour" },
  { value: "24h", label: "24 hours" },
  { value: "7d", label: "7 days" },
  { value: "30d", label: "30 days" },
  { value: "90d", label: "90 days" },
] as const;

function expiryStatus(expiresAt: string | null): "none" | "expired" | "soon" | "ok" {
  if (!expiresAt) return "none";
  try {
    const exp = new Date(expiresAt.replace(" ", "T") + "Z").getTime();
    const now = Date.now();
    if (exp <= now) return "expired";
    if (exp - now < 24 * 60 * 60 * 1000) return "soon";
    return "ok";
  } catch {
    return "none";
  }
}

type Props = {
  onTokenCountChange: (count: number) => void;
};

export default function ApiTokensSection({ onTokenCountChange }: Props) {
  const [apiTokens, setApiTokens] = useState<ApiToken[]>([]);
  const [newTokenName, setNewTokenName] = useState("");
  const [newTokenTtl, setNewTokenTtl] = useState("never");
  const [newTokenRole, setNewTokenRole] = useState("admin");
  const [createdToken, setCreatedToken] = useState<CreatedApiToken | null>(null);
  const [tokenCopied, setTokenCopied] = useState(false);
  const [tokenFeedback, setTokenFeedback] = useState("");
  const [tokenBusy, setTokenBusy] = useState(false);
  const [requireAuthLocalhost, setRequireAuthLocalhost] = useState(false);

  useEffect(() => {
    refreshApiTokens();
    invoke<string | null>("get_setting", { key: "require_auth_localhost" }).then((val) => {
      setRequireAuthLocalhost(val === "true" || val === "1");
    }).catch(() => {});
  }, []);

  const refreshApiTokens = async () => {
    try {
      const tokens = await invoke<ApiToken[]>("list_api_tokens");
      setApiTokens(tokens);
      onTokenCountChange(tokens.length);
    } catch (err) {
      console.error("Failed to load API tokens:", err);
    }
  };

  const createApiToken = async () => {
    const name = newTokenName.trim();
    if (!name) {
      setTokenFeedback("Token name is required");
      setTimeout(() => setTokenFeedback(""), 3000);
      return;
    }
    setTokenBusy(true);
    setTokenFeedback("");
    try {
      const created = await invoke<CreatedApiToken>("create_api_token", {
        name,
        ttl: newTokenTtl === "never" ? null : newTokenTtl,
        role: newTokenRole,
      });
      setCreatedToken(created);
      setNewTokenName("");
      setNewTokenTtl("never");
      setNewTokenRole("admin");
      setTokenCopied(false);
      await refreshApiTokens();
    } catch (err) {
      setTokenFeedback(`Create failed: ${err}`);
    } finally {
      setTokenBusy(false);
    }
  };

  const copyCreatedToken = async () => {
    if (!createdToken) return;
    try {
      await navigator.clipboard.writeText(createdToken.token);
      setTokenCopied(true);
      setTimeout(() => setTokenCopied(false), 2000);
    } catch (err) {
      setTokenFeedback(`Copy failed: ${err}`);
    }
  };

  const dismissCreatedToken = () => {
    setCreatedToken(null);
    setTokenCopied(false);
  };

  const revokeApiToken = async (id: number, name: string) => {
    if (!confirm(`Revoke API token "${name}"? Any client using it will immediately get 401.`)) return;
    try {
      await invoke("revoke_api_token", { id });
      await refreshApiTokens();
      setTokenFeedback("Token revoked");
      setTimeout(() => setTokenFeedback(""), 3000);
    } catch (err) {
      setTokenFeedback(`Revoke failed: ${err}`);
    }
  };

  const toggleRequireAuthLocalhost = async (val: boolean) => {
    setRequireAuthLocalhost(val);
    try {
      await invoke("set_setting", { key: "require_auth_localhost", value: val ? "true" : "false" });
    } catch (err) {
      console.error("Failed to save require_auth_localhost:", err);
      setRequireAuthLocalhost(!val); // revert on failure
    }
  };

  return (
    <>
      <div>
        <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
          API Tokens
        </h2>
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-sm text-zinc-300">
            <Key
              size={16}
              className={apiTokens.length > 0 ? "text-trellis-400" : "text-zinc-500"}
            />
            {apiTokens.length === 0 ? (
              <span className="text-zinc-500">
                No tokens — REST API on <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-xs">:9090</code> rejects all non-loopback requests
              </span>
            ) : (
              <span>
                {apiTokens.length} token{apiTokens.length === 1 ? "" : "s"} active —
                <span className="text-zinc-500"> any non-loopback request must include <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-xs">Authorization: Bearer trls_…</code></span>
              </span>
            )}
          </div>

          {/* Token list */}
          {apiTokens.length > 0 && (
            <div className="border border-zinc-800 rounded-lg overflow-hidden">
              <table className="w-full text-sm">
                <thead className="bg-zinc-900/40 text-zinc-500">
                  <tr>
                    <th className="text-left font-normal px-3 py-2">Name</th>
                    <th className="text-left font-normal px-3 py-2">Role</th>
                    <th className="text-left font-normal px-3 py-2">Created</th>
                    <th className="text-left font-normal px-3 py-2">Expires</th>
                    <th className="text-left font-normal px-3 py-2">Last used</th>
                    <th className="px-3 py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {apiTokens.map((t) => {
                    const status = expiryStatus(t.expires_at);
                    return (
                      <tr key={t.id} className={`border-t border-zinc-800 ${status === "expired" ? "opacity-50" : ""}`}>
                        <td className="px-3 py-2 text-zinc-200">{t.name}</td>
                        <td className="px-3 py-2 text-xs">
                          {t.role === "viewer" ? (
                            <span className="inline-flex items-center gap-1 text-zinc-400">
                              <Eye size={11} /> Viewer
                            </span>
                          ) : (
                            <span className="inline-flex items-center gap-1 text-trellis-400">
                              <Shield size={11} /> Admin
                            </span>
                          )}
                        </td>
                        <td className="px-3 py-2 text-zinc-500 text-xs">{formatTimestamp(t.created_at)}</td>
                        <td className="px-3 py-2 text-xs">
                          {status === "none" && <span className="text-zinc-600">Never</span>}
                          {status === "expired" && (
                            <span className="text-red-400 flex items-center gap-1">
                              <Clock size={11} /> Expired
                            </span>
                          )}
                          {status === "soon" && (
                            <span className="text-amber-400 flex items-center gap-1">
                              <Clock size={11} /> {formatTimestamp(t.expires_at)}
                            </span>
                          )}
                          {status === "ok" && (
                            <span className="text-zinc-500">{formatTimestamp(t.expires_at)}</span>
                          )}
                        </td>
                        <td className="px-3 py-2 text-zinc-500 text-xs">{formatTimestamp(t.last_used_at)}</td>
                        <td className="px-3 py-2 text-right">
                          <button
                            onClick={() => revokeApiToken(t.id, t.name)}
                            className="p-1.5 text-zinc-500 hover:text-red-400 hover:bg-red-500/10 rounded transition-colors"
                            title="Revoke token"
                          >
                            <Trash2 size={14} />
                          </button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}

          {/* Create form */}
          <div className="flex gap-2">
            <input
              type="text"
              value={newTokenName}
              onChange={(e) => setNewTokenName(e.target.value)}
              placeholder="Token name (e.g. homeassistant, phone, ci)"
              className="flex-1 px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-trellis-500"
              onKeyDown={(e) => { if (e.key === "Enter") createApiToken(); }}
              disabled={tokenBusy}
            />
            <select
              value={newTokenTtl}
              onChange={(e) => setNewTokenTtl(e.target.value)}
              className="px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-300 focus:outline-none focus:border-trellis-500"
              disabled={tokenBusy}
            >
              {TTL_OPTIONS.map((o) => (
                <option key={o.value} value={o.value}>{o.label}</option>
              ))}
            </select>
            <select
              value={newTokenRole}
              onChange={(e) => setNewTokenRole(e.target.value)}
              className="px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-300 focus:outline-none focus:border-trellis-500"
              disabled={tokenBusy}
            >
              <option value="admin">Admin</option>
              <option value="viewer">Viewer (read-only)</option>
            </select>
            <button
              onClick={createApiToken}
              disabled={tokenBusy || !newTokenName.trim()}
              className="px-4 py-2 bg-trellis-600 hover:bg-trellis-500 disabled:bg-zinc-700 disabled:opacity-50 text-white rounded-lg text-sm transition-colors"
            >
              Create token
            </button>
          </div>

          {tokenFeedback && (
            <p className={`text-xs ${tokenFeedback.toLowerCase().includes("fail") ? "text-red-400" : "text-trellis-400"}`}>
              {tokenFeedback}
            </p>
          )}

          {/* Strict-loopback toggle */}
          <label className="flex items-start gap-2 text-sm text-zinc-300 pt-2">
            <input
              type="checkbox"
              checked={requireAuthLocalhost}
              onChange={(e) => toggleRequireAuthLocalhost(e.target.checked)}
              className="mt-0.5 rounded border-zinc-700 bg-zinc-800"
            />
            <span>
              Require token even for localhost requests
              <span className="block text-xs text-zinc-600 mt-0.5">
                Default off — the desktop app's embedded dashboard talks to the API over loopback and skipping auth there keeps it friction-free.
                Turn on for defense in depth against malicious local processes.
              </span>
            </span>
          </label>

          <p className="text-xs text-zinc-600">
            Tokens gate the REST API on port 9090. Loopback requests are allowed without a token by default; every other source IP must
            present a valid Bearer token. Tokens are shown exactly once at creation — only the SHA-256 digest is stored, so a stolen
            database can't be used to authenticate.
          </p>
        </div>
      </div>

      {/* Created-token modal — surfaces the plaintext exactly once */}
      {createdToken && (
        <div
          className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
          onClick={dismissCreatedToken}
        >
          <div
            className="bg-zinc-900 border border-zinc-700 rounded-xl max-w-lg w-full p-6 space-y-4"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-start gap-3">
              <div className="p-2 bg-amber-500/10 border border-amber-500/30 rounded-lg">
                <AlertTriangle size={20} className="text-amber-400" />
              </div>
              <div className="flex-1">
                <h3 className="text-lg font-semibold text-zinc-100">Token created — copy it now</h3>
                <p className="text-sm text-zinc-400 mt-1">
                  This is the only time the token will be shown. After you close this dialog,
                  only the digest is kept in the database — there is no way to recover the
                  plaintext.
                </p>
              </div>
            </div>

            <div>
              <div className="flex items-center gap-2 mb-1">
                <label className="text-xs text-zinc-500">{createdToken.name}</label>
                {createdToken.role === "viewer" ? (
                  <span className="inline-flex items-center gap-1 text-xs text-zinc-400 bg-zinc-800 px-1.5 py-0.5 rounded">
                    <Eye size={10} /> Viewer
                  </span>
                ) : (
                  <span className="inline-flex items-center gap-1 text-xs text-trellis-400 bg-trellis-500/10 px-1.5 py-0.5 rounded">
                    <Shield size={10} /> Admin
                  </span>
                )}
              </div>
              <div className="flex gap-2">
                <code className="flex-1 px-3 py-2 bg-zinc-950 border border-zinc-700 rounded-lg text-sm text-amber-300 font-mono break-all">
                  {createdToken.token}
                </code>
                <button
                  onClick={copyCreatedToken}
                  className="px-3 py-2 bg-trellis-600 hover:bg-trellis-500 text-white rounded-lg text-sm transition-colors flex items-center gap-1"
                >
                  {tokenCopied ? <Check size={14} /> : <Copy size={14} />}
                  {tokenCopied ? "Copied" : "Copy"}
                </button>
              </div>
            </div>

            {createdToken.expires_at && (
              <div className="flex items-center gap-2 text-xs text-amber-400/80 bg-amber-500/5 border border-amber-500/20 rounded-lg p-2.5">
                <Clock size={12} />
                Expires {formatTimestamp(createdToken.expires_at)}
              </div>
            )}

            <div className="text-xs text-zinc-500 bg-zinc-800/40 border border-zinc-800 rounded-lg p-3 font-mono break-all">
              curl -H "Authorization: Bearer {createdToken.token}" \<br/>
              &nbsp;&nbsp;http://&lt;host&gt;:9090/api/devices
            </div>

            <div className="flex justify-end">
              <button
                onClick={dismissCreatedToken}
                className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
              >
                I've saved it
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
