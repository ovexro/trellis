//! Remote marketplace catalog fetcher.
//!
//! v0.32.0 slot 1 — opt-in setting that fetches a `marketplace_templates.json`
//! from a configured URL and merges its entries into the in-app catalog
//! alongside the bundled curated set + the user's saved templates.
//!
//! Server-side fetch (not client-side) so the URL stays out of browser logs,
//! the response is cached for `CACHE_TTL` to avoid hammering remote hosts on
//! every page render, and validation against `lib_manifest::current()` runs
//! once. Local always wins on id collision so a remote can't shadow bundled
//! starters. Invalid entries are dropped silently (warn-don't-block at the
//! entry level — a single bad template doesn't poison the catalog).

use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::db::Database;
use crate::lib_manifest;
use crate::marketplace::{self, MarketplaceFile, MarketplaceTemplate};

const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 1_048_576; // 1 MB
const CACHE_TTL: Duration = Duration::from_secs(300); // 5 min

pub const SETTING_KEY_URL: &str = "marketplace_remote_url";
pub const SETTING_KEY_ENABLED: &str = "marketplace_remote_enabled";

#[derive(Debug, Clone, Serialize)]
pub struct RemoteResponse {
    pub enabled: bool,
    pub url: String,
    pub templates: Vec<MarketplaceTemplate>,
    pub error: Option<String>,
    /// Unix seconds when the cached entry was produced. None when disabled or
    /// no fetch has completed yet.
    pub fetched_at: Option<i64>,
}

struct CacheEntry {
    response: RemoteResponse,
    inserted: Instant,
}

fn cache() -> &'static Mutex<Option<CacheEntry>> {
    static CACHE: OnceLock<Mutex<Option<CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

pub fn invalidate_cache() {
    if let Ok(mut g) = cache().lock() {
        *g = None;
    }
}

fn read_setting_bool(db: &Database, key: &str) -> bool {
    matches!(db.get_setting(key), Ok(Some(ref v)) if v == "true")
}

fn read_setting_string(db: &Database, key: &str) -> String {
    db.get_setting(key).ok().flatten().unwrap_or_default()
}

pub fn get_response(db: &Database) -> RemoteResponse {
    let enabled = read_setting_bool(db, SETTING_KEY_ENABLED);
    let url = read_setting_string(db, SETTING_KEY_URL);

    if !enabled {
        return RemoteResponse {
            enabled: false,
            url,
            templates: vec![],
            error: None,
            fetched_at: None,
        };
    }

    if url.trim().is_empty() {
        return RemoteResponse {
            enabled: true,
            url,
            templates: vec![],
            error: Some("Remote marketplace is enabled but no URL is configured.".to_string()),
            fetched_at: None,
        };
    }

    if let Ok(g) = cache().lock() {
        if let Some(entry) = g.as_ref() {
            if entry.inserted.elapsed() < CACHE_TTL && entry.response.url == url {
                return entry.response.clone();
            }
        }
    }

    fetch_and_cache(&url)
}

pub fn refresh(db: &Database) -> RemoteResponse {
    invalidate_cache();
    get_response(db)
}

fn fetch_and_cache(url: &str) -> RemoteResponse {
    let response = match fetch_remote(url) {
        Ok(templates) => RemoteResponse {
            enabled: true,
            url: url.to_string(),
            templates,
            error: None,
            fetched_at: Some(now_unix()),
        },
        Err(e) => RemoteResponse {
            enabled: true,
            url: url.to_string(),
            templates: vec![],
            error: Some(e),
            fetched_at: Some(now_unix()),
        },
    };

    if let Ok(mut g) = cache().lock() {
        *g = Some(CacheEntry {
            response: response.clone(),
            inserted: Instant::now(),
        });
    }

    response
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn fetch_remote(url: &str) -> Result<Vec<MarketplaceTemplate>, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("URL must begin with http:// or https://".to_string());
    }

    let resp = ureq::get(url)
        .timeout(FETCH_TIMEOUT)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, _) => format!("HTTP {}", code),
            ureq::Error::Transport(t) => t.to_string(),
        })?;

    let body = read_bounded(resp.into_reader())?;
    parse_and_filter(&body)
}

fn read_bounded<R: Read>(mut reader: R) -> Result<String, String> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);
    (&mut reader)
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| e.to_string())?;
    if buf.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "Remote manifest exceeds {} bytes",
            MAX_RESPONSE_BYTES
        ));
    }
    String::from_utf8(buf).map_err(|e| e.to_string())
}

/// Parse a remote `marketplace_templates.json` body and filter out invalid
/// entries. Used by `fetch_remote` after the network read; exposed so unit
/// tests can drive validation paths without standing up an HTTP server.
pub fn parse_and_filter(body: &str) -> Result<Vec<MarketplaceTemplate>, String> {
    let parsed: MarketplaceFile = serde_json::from_str(body)
        .map_err(|e| format!("Invalid marketplace JSON: {}", e))?;

    if parsed.marketplace_format != 1 {
        return Err(format!(
            "Unsupported marketplace_format {} (expected 1)",
            parsed.marketplace_format
        ));
    }

    Ok(filter_valid(parsed.templates))
}

/// Drop entries with empty required fields, unsupported boards, or unknown
/// capability kinds. Drop entries whose id collides with a bundled template
/// (local wins) or with another remote entry (first wins). Entry-level
/// warn-don't-block — a malformed template doesn't poison the catalog.
fn filter_valid(remote: Vec<MarketplaceTemplate>) -> Vec<MarketplaceTemplate> {
    let manifest = lib_manifest::current();
    let known_kinds: std::collections::HashSet<&str> =
        manifest.capabilities.iter().map(|s| s.as_str()).collect();
    let known_boards: std::collections::HashSet<&str> =
        manifest.boards.iter().map(|b| b.id.as_str()).collect();
    let local_ids: std::collections::HashSet<&str> =
        marketplace::current().iter().map(|t| t.id.as_str()).collect();

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for t in remote {
        if t.id.is_empty()
            || t.name.is_empty()
            || t.description.is_empty()
            || t.icon.is_empty()
            || t.capabilities.is_empty()
        {
            continue;
        }
        if local_ids.contains(t.id.as_str()) {
            continue;
        }
        if !known_boards.contains(t.board.as_str()) {
            continue;
        }
        if !t
            .capabilities
            .iter()
            .all(|c| known_kinds.contains(c.type_.as_str()))
        {
            continue;
        }
        if !seen.insert(t.id.clone()) {
            continue;
        }
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_valid_remote() -> &'static str {
        r#"{
            "marketplace_format": 1,
            "templates": [
                {
                    "id": "community-buzzer",
                    "name": "Community Buzzer",
                    "description": "Active buzzer toggle.",
                    "icon": "siren",
                    "board": "esp32",
                    "author": "@community",
                    "capabilities": [
                        {"id": "buzzer", "type": "switch", "label": "Buzzer", "gpio": "13", "unit": "", "min": "", "max": ""}
                    ]
                }
            ]
        }"#
    }

    #[test]
    fn parse_and_filter_accepts_valid_remote() {
        let out = parse_and_filter(one_valid_remote()).expect("valid body parses");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "community-buzzer");
    }

    #[test]
    fn parse_and_filter_rejects_unsupported_format() {
        let body = r#"{"marketplace_format": 99, "templates": []}"#;
        let err = parse_and_filter(body).unwrap_err();
        assert!(err.contains("Unsupported marketplace_format"));
    }

    #[test]
    fn parse_and_filter_rejects_invalid_json() {
        let err = parse_and_filter("not json").unwrap_err();
        assert!(err.contains("Invalid marketplace JSON"));
    }

    #[test]
    fn filter_drops_unknown_capability_kind() {
        let body = r#"{
            "marketplace_format": 1,
            "templates": [
                {"id":"x","name":"X","description":"d","icon":"i","board":"esp32","capabilities":[
                    {"id":"weird","type":"flux-capacitor","label":"L","gpio":"","unit":"","min":"","max":""}
                ]}
            ]
        }"#;
        let out = parse_and_filter(body).unwrap();
        assert!(out.is_empty(), "unknown capability kind should drop the entry");
    }

    #[test]
    fn filter_drops_unsupported_board() {
        let body = r#"{
            "marketplace_format": 1,
            "templates": [
                {"id":"x","name":"X","description":"d","icon":"i","board":"avr-uno","capabilities":[
                    {"id":"led","type":"switch","label":"L","gpio":"2","unit":"","min":"","max":""}
                ]}
            ]
        }"#;
        let out = parse_and_filter(body).unwrap();
        assert!(out.is_empty(), "unknown board should drop the entry");
    }

    #[test]
    fn filter_drops_id_collision_with_bundled() {
        // "blink" is one of the bundled starter ids — a remote must not shadow it.
        let body = r#"{
            "marketplace_format": 1,
            "templates": [
                {"id":"blink","name":"Hijack","description":"d","icon":"i","board":"esp32","capabilities":[
                    {"id":"led","type":"switch","label":"L","gpio":"2","unit":"","min":"","max":""}
                ]}
            ]
        }"#;
        let out = parse_and_filter(body).unwrap();
        assert!(out.is_empty(), "remote must not shadow a bundled id");
    }

    #[test]
    fn filter_drops_duplicate_remote_ids() {
        let body = r#"{
            "marketplace_format": 1,
            "templates": [
                {"id":"twin","name":"A","description":"d","icon":"i","board":"esp32","capabilities":[
                    {"id":"led","type":"switch","label":"L","gpio":"2","unit":"","min":"","max":""}
                ]},
                {"id":"twin","name":"B","description":"d","icon":"i","board":"esp32","capabilities":[
                    {"id":"led","type":"switch","label":"L","gpio":"2","unit":"","min":"","max":""}
                ]}
            ]
        }"#;
        let out = parse_and_filter(body).unwrap();
        assert_eq!(out.len(), 1, "first wins on duplicate remote id");
        assert_eq!(out[0].name, "A");
    }

    #[test]
    fn filter_drops_empty_required_fields() {
        let body = r#"{
            "marketplace_format": 1,
            "templates": [
                {"id":"","name":"X","description":"d","icon":"i","board":"esp32","capabilities":[
                    {"id":"led","type":"switch","label":"L","gpio":"2","unit":"","min":"","max":""}
                ]},
                {"id":"x","name":"","description":"d","icon":"i","board":"esp32","capabilities":[
                    {"id":"led","type":"switch","label":"L","gpio":"2","unit":"","min":"","max":""}
                ]},
                {"id":"y","name":"Y","description":"d","icon":"i","board":"esp32","capabilities":[]}
            ]
        }"#;
        let out = parse_and_filter(body).unwrap();
        assert!(out.is_empty(), "every empty-required-field entry must drop");
    }

    #[test]
    fn fetch_rejects_unknown_scheme() {
        let err = fetch_remote("file:///tmp/x.json").unwrap_err();
        assert!(err.contains("must begin with http"));
    }
}
