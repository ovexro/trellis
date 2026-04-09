import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Key, Copy, Trash2, AlertTriangle, Check } from "lucide-react";
import type { ApiToken, CreatedApiToken } from "./types";
import { formatTimestamp } from "./types";

type Props = {
  onTokenCountChange: (count: number) => void;
};

export default function ApiTokensSection({ onTokenCountChange }: Props) {
  const [apiTokens, setApiTokens] = useState<ApiToken[]>([]);
  const [newTokenName, setNewTokenName] = useState("");
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
      const created = await invoke<CreatedApiToken>("create_api_token", { name });
      setCreatedToken(created);
      setNewTokenName("");
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
                    <th className="text-left font-normal px-3 py-2">Created</th>
                    <th className="text-left font-normal px-3 py-2">Last used</th>
                    <th className="px-3 py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {apiTokens.map((t) => (
                    <tr key={t.id} className="border-t border-zinc-800">
                      <td className="px-3 py-2 text-zinc-200">{t.name}</td>
                      <td className="px-3 py-2 text-zinc-500 text-xs">{formatTimestamp(t.created_at)}</td>
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
                  ))}
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
              <label className="text-xs text-zinc-500 block mb-1">{createdToken.name}</label>
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
