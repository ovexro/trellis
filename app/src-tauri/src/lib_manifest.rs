use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use crate::sketch_gen::{CapabilitySpec, SketchSpec};

/// Bundled at build time. The repo-root `lib_manifest.json` is the source of
/// truth for what capability kinds + boards the Trellis Arduino library
/// supports at this version. Embedding it via `include_str!` means the desktop
/// binary always ships with the manifest that matches `library.properties` at
/// build time, so the sketch generator can never offer a kind the library
/// doesn't actually have.
const MANIFEST_JSON: &str = include_str!("../../../lib_manifest.json");

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LibManifest {
    pub library: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub boards: Vec<BoardEntry>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoardEntry {
    pub id: String,
    pub label: String,
}

pub fn current() -> &'static LibManifest {
    static CURRENT: OnceLock<LibManifest> = OnceLock::new();
    CURRENT.get_or_init(|| {
        serde_json::from_str(MANIFEST_JSON)
            .expect("lib_manifest.json embedded at build time must be valid JSON")
    })
}

fn capability_kind(cap: &CapabilitySpec) -> &'static str {
    match cap {
        CapabilitySpec::Switch { .. } => "switch",
        CapabilitySpec::Slider { .. } => "slider",
        CapabilitySpec::Color { .. } => "color",
        CapabilitySpec::Sensor { .. } => "sensor",
        CapabilitySpec::Text { .. } => "text",
    }
}

/// Reject a sketch spec carrying capability kinds the bundled library doesn't
/// support. Defense-in-depth — the wizard UI already filters its capability
/// buttons by the manifest, but a spec posted directly to the REST endpoint
/// or restored from a saved template might carry stale kinds.
pub fn validate_capability_kinds(spec: &SketchSpec) -> Result<(), String> {
    let manifest = current();
    for cap in &spec.capabilities {
        let kind = capability_kind(cap);
        if !manifest.capabilities.iter().any(|k| k == kind) {
            return Err(format!(
                "capability kind '{}' is not supported by Trellis library {}",
                kind, manifest.version
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch_gen::{BoardKind, CapabilitySpec, SketchSpec};

    #[test]
    fn manifest_loads_at_startup() {
        let m = current();
        assert_eq!(m.library, "Trellis");
        assert!(!m.version.is_empty());
        assert!(!m.capabilities.is_empty());
        assert!(!m.boards.is_empty());
    }

    /// Regression guard: when a new variant is added to `CapabilitySpec`, the
    /// manifest must be updated to match. This test constructs one of every
    /// variant and asserts the bundled manifest accepts it. If a variant is
    /// added without updating `lib_manifest.json`, this test fails — exactly
    /// the version-skew gap the manifest exists to close.
    #[test]
    fn manifest_includes_all_capabilityspec_variants() {
        let all_kinds = vec![
            CapabilitySpec::Switch {
                id: "s".into(),
                label: "S".into(),
                gpio: 2,
            },
            CapabilitySpec::Slider {
                id: "l".into(),
                label: "L".into(),
                gpio: 5,
                min: 0.0,
                max: 100.0,
            },
            CapabilitySpec::Color {
                id: "c".into(),
                label: "C".into(),
            },
            CapabilitySpec::Sensor {
                id: "n".into(),
                label: "N".into(),
                unit: "C".into(),
                gpio: None,
            },
            CapabilitySpec::Text {
                id: "t".into(),
                label: "T".into(),
            },
        ];
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            board: Some(BoardKind::Esp32),
            capabilities: all_kinds,
        };
        validate_capability_kinds(&spec).expect("manifest must accept every CapabilitySpec variant");
    }

    #[test]
    fn manifest_lists_both_supported_boards() {
        let ids: Vec<&str> = current().boards.iter().map(|b| b.id.as_str()).collect();
        assert!(ids.contains(&"esp32"));
        assert!(ids.contains(&"picow"));
    }

    #[test]
    fn validate_capability_kinds_accepts_known_kind() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            board: None,
            capabilities: vec![CapabilitySpec::Switch {
                id: "led".into(),
                label: "LED".into(),
                gpio: 2,
            }],
        };
        assert!(validate_capability_kinds(&spec).is_ok());
    }
}
