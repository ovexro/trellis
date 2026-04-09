// REST API authentication for the :9090 server.
//
// What this protects: every endpoint exposed by api.rs against unauthorized
// LAN access. The REST server binds to 0.0.0.0 so anyone on the same WiFi
// could (until v0.3.4) curl `/api/devices/foo/command` and flip switches,
// drain sensor history, etc. v0.3.3 redacted the MQTT password from one
// specific GET response — this module gates the entire surface behind a
// Bearer token.
//
// Threat model:
//   - In scope: passive network attackers on the same LAN, malicious
//     guests on the home WiFi, browser-based mistakes ("I curl'd the
//     wrong machine"), accidental port-forward exposure.
//   - Out of scope: a malicious local process running as the same user
//     (it has filesystem access to the SQLite token table and can mint its
//     own tokens via the Tauri commands; this is the same trust boundary
//     as secret_store.rs).
//
// Token shape: `trls_<43 chars of base64url-no-pad>`
//   - 32 bytes from the OS RNG (`OsRng`) → base64url-no-pad → 43 chars.
//   - `trls_` prefix mirrors `ghp_` etc. — makes it greppable in logs and
//     trivially distinguishable from other secrets in a config file.
//   - Total length 48 chars; fits in any sensible max-header-size.
//
// Storage: only the SHA-256 hex digest of the full token (prefix included)
// hits SQLite. The plaintext is shown to the user exactly once at create
// time and never persisted. This means a stolen DB cannot be used to make
// authenticated requests — the attacker would need to brute-force a
// 256-bit secret, which they cannot.
//
// Auth gate logic:
//   1. CORS preflight (OPTIONS): always allowed (no credentials needed for
//      browser preflight; the actual request still gets gated).
//   2. Loopback request (127.0.0.1, ::1):
//        - if `require_auth_localhost` setting is true: enforce token
//        - else (default): allow without token
//      The default bypass keeps the embedded HTML dashboard at
//      `localhost:9090/` and any user-run local tooling (curl, scripts,
//      home-grown wrappers) working with zero friction. The Trellis
//      desktop app itself does NOT depend on the loopback bypass — its
//      React frontend talks to the Rust backend over Tauri IPC, not over
//      HTTP. The strict-loopback opt-in is for users who want defense in
//      depth against malicious local processes on a multi-user machine.
//   3. Non-loopback request: always require a valid token. No exception.
//      First-time users get a distinct error message that points them at
//      the Settings UI to mint one.
//
// Why this is in its own module instead of inlined into api.rs: the auth
// helpers are also called by the Tauri commands (create/list/revoke), so
// keeping the token-generation and hashing logic here lets both the
// command layer and the HTTP middleware share one implementation. There's
// only one source of truth for "how do we mint a token" and "how do we
// hash one for storage".

use std::net::SocketAddr;

use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

use crate::db::Database;

/// Prefix for every Trellis API token. Used by humans (greppable, easy to
/// spot in logs/config files) and by sanity checks in the auth middleware
/// (a value missing this prefix is rejected before even hashing it, so we
/// don't waste a SHA-256 on an obviously-wrong input).
pub const TOKEN_PREFIX: &str = "trls_";

/// Number of random bytes used to seed each token. 32 bytes = 256 bits of
/// entropy from the OS RNG, which is the same security level as the keys
/// the rest of the app generates.
const TOKEN_BYTE_LEN: usize = 32;

/// Setting key for the "require token even on loopback" toggle. Default
/// is `false` — loopback requests bypass auth so the desktop app's
/// embedded WebView and any local CLI work with no setup.
pub const REQUIRE_AUTH_LOCALHOST_KEY: &str = "require_auth_localhost";

/// Outcome of an auth check. Either the request is authorized to proceed
/// (with an optional token row id for `last_used_at` bookkeeping), or it
/// must be rejected with the contained status + error body.
pub enum AuthResult {
    /// Request may proceed. `Some(id)` means a token was used and its
    /// last-used timestamp should be touched. `None` means the request
    /// was bypassed (loopback default) and there's no token to update.
    Allow(Option<i64>),
    /// Request must be rejected. `(status_code, error_message_for_body)`.
    /// The middleware turns this into a JSON 401/403 response.
    Deny(u16, String),
}

/// Generate a fresh API token. Returns `(plaintext, sha256_hex)`. The
/// plaintext is what the user copies into their curl command; the hex
/// digest is what gets stored in `api_tokens.token_hash`.
///
/// This is the **only** place in the codebase where the plaintext form
/// exists for more than the duration of an inbound request — the caller
/// must echo it to the UI immediately and then drop it.
pub fn generate_token() -> (String, String) {
    let mut bytes = [0u8; TOKEN_BYTE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let plaintext = format!("{}{}", TOKEN_PREFIX, body);
    let hash = sha256_hex(&plaintext);
    (plaintext, hash)
}

/// SHA-256 of a token, as lowercase hex. Used both at create time (to
/// store the digest) and at request time (to look up an incoming token in
/// the table). Sha-256 is collision-resistant and the input has 256 bits
/// of entropy, so a successful match means the caller knows the plaintext.
pub fn sha256_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// True if the peer address is on the loopback interface (IPv4 127.0.0.0/8
/// or IPv6 ::1). Loopback is the default-bypass branch of the auth gate;
/// every other source IP is treated as remote and must present a token.
pub fn is_loopback(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

/// Run the auth check for an incoming request. The middleware in api.rs
/// calls this once per request, before route dispatch.
///
/// Inputs:
///   - `db` — for token lookup + count + touch
///   - `peer_addr` — used to decide loopback vs remote
///   - `auth_header` — the raw `Authorization:` header value, if any
///   - `require_auth_localhost` — value of the setting (already loaded by
///     the caller; we don't query SQLite twice per request)
///
/// Returns `AuthResult::Allow(token_id)` on success or
/// `AuthResult::Deny(status, body)` with a friendly error message that
/// the middleware can drop into a JSON response.
pub fn check_auth(
    db: &Database,
    peer_addr: &SocketAddr,
    auth_header: Option<&str>,
    require_auth_localhost: bool,
) -> AuthResult {
    let loopback = is_loopback(peer_addr);

    // Branch 1: loopback default-bypass
    if loopback && !require_auth_localhost {
        return AuthResult::Allow(None);
    }

    // Branch 2: token required (either remote, or loopback with strict mode)
    let token = match extract_bearer(auth_header) {
        Some(t) => t,
        None => {
            // Distinct messages for first-time users vs misconfigured ones.
            // The "no tokens minted yet" hint is a much more helpful UX than
            // a generic "missing Authorization header".
            let count = db.count_api_tokens().unwrap_or(0);
            let msg = if count == 0 {
                "Authentication required. Open Trellis → Settings → API Tokens and click Create to mint one, then send it as `Authorization: Bearer <token>`.".to_string()
            } else {
                "Missing or malformed Authorization header. Expected `Authorization: Bearer trls_...`.".to_string()
            };
            return AuthResult::Deny(401, msg);
        }
    };

    // Sanity-check the prefix before hashing — saves a SHA-256 round when a
    // caller obviously sent the wrong thing (e.g. a stale OAuth token from
    // some other service). Not a security check, just an early-out.
    if !token.starts_with(TOKEN_PREFIX) {
        return AuthResult::Deny(
            401,
            format!("Invalid token format. Trellis tokens start with `{}`.", TOKEN_PREFIX),
        );
    }

    let hash = sha256_hex(&token);
    match db.find_api_token_by_hash(&hash) {
        Ok(Some((id, expires_at))) => {
            if let Some(ref exp) = expires_at {
                if is_expired(exp) {
                    return AuthResult::Deny(
                        401,
                        "API token has expired. Mint a new one in Settings → API Tokens.".to_string(),
                    );
                }
            }
            AuthResult::Allow(Some(id))
        }
        Ok(None) => AuthResult::Deny(401, "Invalid or revoked API token.".to_string()),
        Err(e) => {
            log::error!("[Auth] Token lookup failed: {}", e);
            AuthResult::Deny(500, "Internal authentication error.".to_string())
        }
    }
}

/// Check whether an `expires_at` timestamp (SQLite datetime format,
/// `"YYYY-MM-DD HH:MM:SS"` in UTC) is in the past.
fn is_expired(expires_at: &str) -> bool {
    use chrono::{NaiveDateTime, Utc};
    match NaiveDateTime::parse_from_str(expires_at, "%Y-%m-%d %H:%M:%S") {
        Ok(exp) => Utc::now().naive_utc() > exp,
        Err(_) => false, // unparseable → treat as non-expired (fail open, not closed)
    }
}

/// Compute an `expires_at` timestamp from a TTL string like `"1h"`, `"24h"`,
/// `"7d"`, `"30d"`. Returns `None` for `"never"` or unrecognized input.
/// The result is a UTC datetime string in SQLite format.
pub fn compute_expires_at(ttl: &str) -> Option<String> {
    use chrono::{Duration, Utc};
    let duration = match ttl.trim() {
        "1h" => Duration::hours(1),
        "24h" => Duration::hours(24),
        "7d" => Duration::days(7),
        "30d" => Duration::days(30),
        "90d" => Duration::days(90),
        "never" | "" => return None,
        _ => return None,
    };
    let exp = Utc::now() + duration;
    Some(exp.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Pull the token out of an `Authorization: Bearer <token>` header value.
/// Case-insensitive on the scheme name (RFC 7235 §2.1) but the token body
/// itself is left as-is — base64url is case-sensitive.
fn extract_bearer(header: Option<&str>) -> Option<String> {
    let raw = header?.trim();
    let mut parts = raw.splitn(2, char::is_whitespace);
    let scheme = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = parts.next()?.trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_has_prefix_and_expected_length() {
        let (plain, hash) = generate_token();
        assert!(plain.starts_with(TOKEN_PREFIX));
        // 32 bytes → 43 chars base64url-no-pad → + 5-char prefix = 48
        assert_eq!(plain.len(), 48);
        // SHA-256 hex is 64 chars
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn two_tokens_differ() {
        let (a, _) = generate_token();
        let (b, _) = generate_token();
        assert_ne!(a, b);
    }

    #[test]
    fn hash_is_stable() {
        // Same input → same output, every time. This is what makes the
        // SQLite lookup work.
        assert_eq!(sha256_hex("trls_xyz"), sha256_hex("trls_xyz"));
        assert_ne!(sha256_hex("trls_a"), sha256_hex("trls_b"));
    }

    #[test]
    fn extract_bearer_happy_path() {
        assert_eq!(
            extract_bearer(Some("Bearer trls_abc")),
            Some("trls_abc".to_string())
        );
        // Case-insensitive scheme
        assert_eq!(
            extract_bearer(Some("bearer trls_abc")),
            Some("trls_abc".to_string())
        );
        assert_eq!(
            extract_bearer(Some("BEARER trls_abc")),
            Some("trls_abc".to_string())
        );
    }

    #[test]
    fn extract_bearer_rejects_other_schemes() {
        assert_eq!(extract_bearer(Some("Basic dXNlcjpwYXNz")), None);
        assert_eq!(extract_bearer(Some("Token trls_abc")), None);
        assert_eq!(extract_bearer(None), None);
        assert_eq!(extract_bearer(Some("")), None);
        assert_eq!(extract_bearer(Some("Bearer ")), None);
    }

    #[test]
    fn is_expired_checks() {
        use chrono::{Duration, Utc};
        let past = (Utc::now() - Duration::hours(1)).format("%Y-%m-%d %H:%M:%S").to_string();
        let future = (Utc::now() + Duration::hours(1)).format("%Y-%m-%d %H:%M:%S").to_string();
        assert!(is_expired(&past));
        assert!(!is_expired(&future));
        // Unparseable → not expired (fail open)
        assert!(!is_expired("garbage"));
    }

    #[test]
    fn compute_expires_at_values() {
        assert!(compute_expires_at("1h").is_some());
        assert!(compute_expires_at("24h").is_some());
        assert!(compute_expires_at("7d").is_some());
        assert!(compute_expires_at("30d").is_some());
        assert!(compute_expires_at("90d").is_some());
        assert!(compute_expires_at("never").is_none());
        assert!(compute_expires_at("").is_none());
        assert!(compute_expires_at("bogus").is_none());
    }

    #[test]
    fn loopback_detection() {
        let v4: SocketAddr = "127.0.0.1:9090".parse().unwrap();
        let v4_high: SocketAddr = "127.5.5.5:9090".parse().unwrap();
        let v6: SocketAddr = "[::1]:9090".parse().unwrap();
        let lan: SocketAddr = "192.168.1.50:9090".parse().unwrap();
        assert!(is_loopback(&v4));
        assert!(is_loopback(&v4_high));
        assert!(is_loopback(&v6));
        assert!(!is_loopback(&lan));
    }
}
