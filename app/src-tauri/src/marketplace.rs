use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Bundled at build time. The repo-root `marketplace_templates.json` is the
/// canonical curated catalog of starter sketches the desktop wizard and the
/// `:9090` Sketch tab offer as one-click starting points. Embedding via
/// `include_str!` means the binary always ships with the catalog that matches
/// the bundled `lib_manifest.json` — no runtime fetch, no version skew.
const MARKETPLACE_JSON: &str = include_str!("../../../marketplace_templates.json");

const MARKETPLACE_FORMAT: u32 = 1;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarketplaceFile {
    pub marketplace_format: u32,
    pub templates: Vec<MarketplaceTemplate>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarketplaceTemplate {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub board: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub capabilities: Vec<MarketplaceCapability>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MarketplaceCapability {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub label: String,
    pub gpio: String,
    pub unit: String,
    pub min: String,
    pub max: String,
}

pub fn current() -> &'static [MarketplaceTemplate] {
    static CURRENT: OnceLock<Vec<MarketplaceTemplate>> = OnceLock::new();
    CURRENT.get_or_init(|| {
        let parsed: MarketplaceFile = serde_json::from_str(MARKETPLACE_JSON)
            .expect("marketplace_templates.json embedded at build time must be valid JSON");
        assert_eq!(
            parsed.marketplace_format, MARKETPLACE_FORMAT,
            "marketplace_format mismatch — embedded JSON must declare format {}",
            MARKETPLACE_FORMAT
        );
        parsed.templates
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lib_manifest;

    #[test]
    fn marketplace_loads_at_startup() {
        let templates = current();
        assert!(!templates.is_empty(), "marketplace must ship with at least one template");
    }

    #[test]
    fn marketplace_includes_expected_starter_set() {
        let ids: Vec<&str> = current().iter().map(|t| t.id.as_str()).collect();
        for expected in &["blink", "sensor-monitor", "smart-relay", "weather-station", "greenhouse"] {
            assert!(ids.contains(expected), "missing expected starter template: {}", expected);
        }
    }

    #[test]
    fn marketplace_template_ids_are_unique() {
        let templates = current();
        let mut seen = std::collections::HashSet::new();
        for t in templates {
            assert!(seen.insert(t.id.as_str()), "duplicate template id: {}", t.id);
        }
    }

    /// Regression guard: every bundled template's capability kinds and board
    /// must be accepted by `lib_manifest.json`. If a template references a
    /// capability or board the library doesn't declare, this test fails before
    /// shipping — the same version-skew gap `lib_manifest` closes for the
    /// generator extends to the marketplace catalog.
    #[test]
    fn every_marketplace_template_validates_against_lib_manifest() {
        let manifest = lib_manifest::current();
        let known_kinds: std::collections::HashSet<&str> =
            manifest.capabilities.iter().map(|s| s.as_str()).collect();
        let known_boards: std::collections::HashSet<&str> =
            manifest.boards.iter().map(|b| b.id.as_str()).collect();

        for t in current() {
            assert!(
                known_boards.contains(t.board.as_str()),
                "template '{}' references unsupported board '{}'",
                t.id,
                t.board
            );
            for c in &t.capabilities {
                assert!(
                    known_kinds.contains(c.type_.as_str()),
                    "template '{}' capability '{}' has unsupported kind '{}'",
                    t.id,
                    c.id,
                    c.type_
                );
            }
        }
    }

    #[test]
    fn marketplace_template_required_fields_non_empty() {
        for t in current() {
            assert!(!t.id.is_empty(), "template id must not be empty");
            assert!(!t.name.is_empty(), "template '{}' name must not be empty", t.id);
            assert!(
                !t.description.is_empty(),
                "template '{}' description must not be empty",
                t.id
            );
            assert!(!t.icon.is_empty(), "template '{}' icon must not be empty", t.id);
            assert!(
                !t.capabilities.is_empty(),
                "template '{}' must declare at least one capability",
                t.id
            );
        }
    }
}
