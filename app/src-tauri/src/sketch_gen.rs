use serde::{Deserialize, Serialize};

const MAX_CAPABILITIES: usize = 16;
const MAX_NAME_LEN: usize = 64;
const MAX_ID_LEN: usize = 32;
const MAX_LABEL_LEN: usize = 64;
const MAX_UNIT_LEN: usize = 16;
const GPIO_MIN: i32 = 0;
const GPIO_MAX: i32 = 48;

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CapabilitySpec {
    Switch {
        id: String,
        label: String,
        gpio: i32,
    },
    Slider {
        id: String,
        label: String,
        gpio: i32,
        min: f32,
        max: f32,
    },
    Color {
        id: String,
        label: String,
    },
    Sensor {
        id: String,
        label: String,
        unit: String,
    },
}

impl CapabilitySpec {
    fn id(&self) -> &str {
        match self {
            CapabilitySpec::Switch { id, .. }
            | CapabilitySpec::Slider { id, .. }
            | CapabilitySpec::Color { id, .. }
            | CapabilitySpec::Sensor { id, .. } => id,
        }
    }

    fn label(&self) -> &str {
        match self {
            CapabilitySpec::Switch { label, .. }
            | CapabilitySpec::Slider { label, .. }
            | CapabilitySpec::Color { label, .. }
            | CapabilitySpec::Sensor { label, .. } => label,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SketchSpec {
    pub device_name: String,
    #[serde(default)]
    pub firmware_version: Option<String>,
    pub capabilities: Vec<CapabilitySpec>,
}

fn validate_identifier(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("identifier must not be empty".into());
    }
    if id.len() > MAX_ID_LEN {
        return Err(format!(
            "identifier '{}' exceeds {} chars",
            id, MAX_ID_LEN
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!(
            "identifier '{}' must match [a-zA-Z0-9_]+",
            id
        ));
    }
    Ok(())
}

fn validate_string_literal(value: &str, field: &str, max_len: usize) -> Result<(), String> {
    if value.len() > max_len {
        return Err(format!("{} exceeds {} chars", field, max_len));
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(format!("{} must not contain newlines", field));
    }
    if value.contains('"') || value.contains('\\') {
        return Err(format!("{} must not contain quotes or backslashes", field));
    }
    Ok(())
}

fn validate_device_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("device_name must not be empty".into());
    }
    validate_string_literal(name, "device_name", MAX_NAME_LEN)
}

fn validate_firmware_version(v: &str) -> Result<(), String> {
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() != 3 || !parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())) {
        return Err(format!("firmware_version '{}' must be N.N.N", v));
    }
    Ok(())
}

fn validate_gpio(gpio: i32) -> Result<(), String> {
    if !(GPIO_MIN..=GPIO_MAX).contains(&gpio) {
        return Err(format!(
            "gpio {} out of range ({}..={})",
            gpio, GPIO_MIN, GPIO_MAX
        ));
    }
    Ok(())
}

fn validate_capability(cap: &CapabilitySpec) -> Result<(), String> {
    validate_identifier(cap.id())?;
    validate_string_literal(cap.label(), "label", MAX_LABEL_LEN)?;
    if cap.label().is_empty() {
        return Err("label must not be empty".into());
    }
    match cap {
        CapabilitySpec::Switch { gpio, .. } => validate_gpio(*gpio),
        CapabilitySpec::Slider { gpio, min, max, .. } => {
            validate_gpio(*gpio)?;
            if !min.is_finite() || !max.is_finite() {
                return Err("slider min/max must be finite".into());
            }
            if min >= max {
                return Err(format!("slider min ({}) must be less than max ({})", min, max));
            }
            Ok(())
        }
        CapabilitySpec::Color { .. } => Ok(()),
        CapabilitySpec::Sensor { unit, .. } => {
            validate_string_literal(unit, "sensor unit", MAX_UNIT_LEN)
        }
    }
}

fn validate_spec(spec: &SketchSpec) -> Result<(), String> {
    validate_device_name(&spec.device_name)?;
    if let Some(ref v) = spec.firmware_version {
        validate_firmware_version(v)?;
    }
    if spec.capabilities.len() > MAX_CAPABILITIES {
        return Err(format!(
            "too many capabilities ({}); max is {}",
            spec.capabilities.len(),
            MAX_CAPABILITIES
        ));
    }
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for cap in &spec.capabilities {
        validate_capability(cap)?;
        if !seen.insert(cap.id()) {
            return Err(format!("duplicate capability id '{}'", cap.id()));
        }
    }
    Ok(())
}

fn fmt_float(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

pub fn generate(spec: &SketchSpec) -> Result<String, String> {
    validate_spec(spec)?;

    let firmware = spec.firmware_version.as_deref().unwrap_or("1.0.0");
    let needs_on_command = spec
        .capabilities
        .iter()
        .any(|c| matches!(c, CapabilitySpec::Slider { .. } | CapabilitySpec::Color { .. }));
    let has_sensor = spec
        .capabilities
        .iter()
        .any(|c| matches!(c, CapabilitySpec::Sensor { .. }));

    let mut out = String::new();
    out.push_str(&format!(
        "/*\n * Generated by Trellis Sketch Generator\n * Device: {}\n *\n * Replace WIFI_SSID/WIFI_PASS below with your network credentials,\n * then upload to your ESP32 or Pico W.\n *\n * Works on: ESP32, Pico W, Pico 2 W\n */\n\n",
        spec.device_name
    ));
    out.push_str("#include <Trellis.h>\n\n");
    out.push_str("const char* WIFI_SSID = \"YourWiFi\";\n");
    out.push_str("const char* WIFI_PASS = \"YourPassword\";\n\n");
    out.push_str(&format!("Trellis trellis(\"{}\");\n\n", spec.device_name));

    if needs_on_command {
        out.push_str("// Fires when the user controls a slider/color from the app or dashboard.\n");
        out.push_str("// Each branch is a stub — wire it to your hardware.\n");
        out.push_str("void onCommand(const char* id, JsonVariant value) {\n");
        for cap in &spec.capabilities {
            match cap {
                CapabilitySpec::Slider { id, label, .. } => {
                    out.push_str(&format!("  if (strcmp(id, \"{}\") == 0) {{\n", id));
                    out.push_str("    float v = value.as<float>();\n");
                    out.push_str(&format!("    Serial.printf(\"{}: %.2f\\n\", v);\n", label));
                    out.push_str("    // TODO: drive your hardware (analogWrite, etc.) using v\n");
                    out.push_str("    return;\n");
                    out.push_str("  }\n");
                }
                CapabilitySpec::Color { id, label } => {
                    out.push_str(&format!("  if (strcmp(id, \"{}\") == 0) {{\n", id));
                    out.push_str("    const char* hex = value.as<const char*>();\n");
                    out.push_str("    if (!hex || strlen(hex) < 7) return;\n");
                    out.push_str("    long color = strtol(hex + 1, NULL, 16);\n");
                    out.push_str("    int r = (color >> 16) & 0xFF;\n");
                    out.push_str("    int g = (color >> 8) & 0xFF;\n");
                    out.push_str("    int b = color & 0xFF;\n");
                    out.push_str(&format!(
                        "    Serial.printf(\"{}: R=%d G=%d B=%d\\n\", r, g, b);\n",
                        label
                    ));
                    out.push_str("    // TODO: drive your RGB pins using r, g, b\n");
                    out.push_str("    return;\n");
                    out.push_str("  }\n");
                }
                _ => {}
            }
        }
        out.push_str("}\n\n");
    }

    out.push_str("void setup() {\n");
    out.push_str("  Serial.begin(115200);\n");
    out.push_str(&format!("  trellis.setFirmwareVersion(\"{}\");\n", firmware));
    for cap in &spec.capabilities {
        match cap {
            CapabilitySpec::Switch { id, label, gpio } => {
                out.push_str(&format!(
                    "  trellis.addSwitch(\"{}\", \"{}\", {});\n",
                    id, label, gpio
                ));
            }
            CapabilitySpec::Slider {
                id,
                label,
                gpio,
                min,
                max,
            } => {
                out.push_str(&format!(
                    "  trellis.addSlider(\"{}\", \"{}\", {}, {}, {});\n",
                    id,
                    label,
                    fmt_float(*min),
                    fmt_float(*max),
                    gpio
                ));
            }
            CapabilitySpec::Color { id, label } => {
                out.push_str(&format!(
                    "  trellis.addColor(\"{}\", \"{}\");\n",
                    id, label
                ));
            }
            CapabilitySpec::Sensor { id, label, unit } => {
                out.push_str(&format!(
                    "  trellis.addSensor(\"{}\", \"{}\", \"{}\");\n",
                    id, label, unit
                ));
            }
        }
    }
    if needs_on_command {
        out.push_str("  trellis.onCommand(onCommand);\n");
    }
    out.push_str("  trellis.begin(WIFI_SSID, WIFI_PASS);\n");
    out.push_str("}\n\n");

    out.push_str("void loop() {\n");
    out.push_str("  trellis.loop();\n");
    if has_sensor {
        out.push_str("  // TODO: read your sensor(s) and call trellis.setSensor(\"<id>\", value)\n");
        out.push_str("  // periodically — e.g. once per second.\n");
    }
    out.push_str("}\n");

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn switch_cap(id: &str) -> CapabilitySpec {
        CapabilitySpec::Switch {
            id: id.into(),
            label: "Lamp".into(),
            gpio: 2,
        }
    }

    #[test]
    fn empty_capabilities_produces_minimal_sketch() {
        let spec = SketchSpec {
            device_name: "Empty".into(),
            firmware_version: None,
            capabilities: vec![],
        };
        let out = generate(&spec).unwrap();
        assert!(out.contains("Trellis trellis(\"Empty\");"));
        assert!(out.contains("void setup()"));
        assert!(out.contains("void loop()"));
        assert!(!out.contains("onCommand"));
        assert!(!out.contains("trellis.add"));
        assert!(out.contains("trellis.setFirmwareVersion(\"1.0.0\");"));
    }

    #[test]
    fn all_four_kinds_emits_expected_calls_and_handler() {
        let spec = SketchSpec {
            device_name: "Test Device".into(),
            firmware_version: Some("2.3.4".into()),
            capabilities: vec![
                CapabilitySpec::Switch {
                    id: "led".into(),
                    label: "LED".into(),
                    gpio: 2,
                },
                CapabilitySpec::Slider {
                    id: "dim".into(),
                    label: "Dim".into(),
                    gpio: 5,
                    min: 0.0,
                    max: 100.0,
                },
                CapabilitySpec::Color {
                    id: "rgb".into(),
                    label: "RGB".into(),
                },
                CapabilitySpec::Sensor {
                    id: "temp".into(),
                    label: "Temperature".into(),
                    unit: "C".into(),
                },
            ],
        };
        let out = generate(&spec).unwrap();
        assert!(out.contains("trellis.addSwitch(\"led\", \"LED\", 2);"));
        assert!(out.contains("trellis.addSlider(\"dim\", \"Dim\", 0, 100, 5);"));
        assert!(out.contains("trellis.addColor(\"rgb\", \"RGB\");"));
        assert!(out.contains("trellis.addSensor(\"temp\", \"Temperature\", \"C\");"));
        assert!(out.contains("trellis.onCommand(onCommand);"));
        assert!(out.contains("if (strcmp(id, \"dim\") == 0)"));
        assert!(out.contains("if (strcmp(id, \"rgb\") == 0)"));
        assert!(out.contains("trellis.setFirmwareVersion(\"2.3.4\");"));
        assert!(out.contains("// TODO: read your sensor(s)"));
    }

    #[test]
    fn switch_only_skips_on_command() {
        let spec = SketchSpec {
            device_name: "SwitchOnly".into(),
            firmware_version: None,
            capabilities: vec![switch_cap("led")],
        };
        let out = generate(&spec).unwrap();
        assert!(!out.contains("onCommand"));
        assert!(!out.contains("trellis.onCommand"));
    }

    #[test]
    fn sensor_only_skips_on_command_and_includes_loop_hint() {
        let spec = SketchSpec {
            device_name: "SensorOnly".into(),
            firmware_version: None,
            capabilities: vec![CapabilitySpec::Sensor {
                id: "t".into(),
                label: "T".into(),
                unit: "C".into(),
            }],
        };
        let out = generate(&spec).unwrap();
        assert!(!out.contains("onCommand"));
        assert!(out.contains("trellis.setSensor"));
    }

    #[test]
    fn slider_with_min_geq_max_rejects() {
        let spec = SketchSpec {
            device_name: "Bad".into(),
            firmware_version: None,
            capabilities: vec![CapabilitySpec::Slider {
                id: "s".into(),
                label: "S".into(),
                gpio: 5,
                min: 100.0,
                max: 50.0,
            }],
        };
        assert!(generate(&spec).unwrap_err().contains("less than max"));
    }

    #[test]
    fn capability_id_with_whitespace_rejects() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: vec![switch_cap("bad id")],
        };
        assert!(generate(&spec).unwrap_err().contains("[a-zA-Z0-9_]+"));
    }

    #[test]
    fn capability_id_with_punctuation_rejects() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: vec![switch_cap("foo;system(\"rm\")")],
        };
        assert!(generate(&spec).is_err());
    }

    #[test]
    fn label_with_quote_rejects() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: vec![CapabilitySpec::Switch {
                id: "led".into(),
                label: "He said \"hi\"".into(),
                gpio: 2,
            }],
        };
        assert!(generate(&spec).unwrap_err().contains("quotes"));
    }

    #[test]
    fn label_with_newline_rejects() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: vec![CapabilitySpec::Switch {
                id: "led".into(),
                label: "line1\nline2".into(),
                gpio: 2,
            }],
        };
        assert!(generate(&spec).unwrap_err().contains("newlines"));
    }

    #[test]
    fn device_name_with_newline_rejects() {
        let spec = SketchSpec {
            device_name: "Bad\nName".into(),
            firmware_version: None,
            capabilities: vec![],
        };
        assert!(generate(&spec).unwrap_err().contains("device_name"));
    }

    #[test]
    fn duplicate_capability_ids_reject() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: vec![switch_cap("led"), switch_cap("led")],
        };
        assert!(generate(&spec).unwrap_err().contains("duplicate"));
    }

    #[test]
    fn too_many_capabilities_rejects() {
        let caps: Vec<CapabilitySpec> = (0..17).map(|i| switch_cap(&format!("c{}", i))).collect();
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: caps,
        };
        assert!(generate(&spec).unwrap_err().contains("too many"));
    }

    #[test]
    fn gpio_out_of_range_rejects() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: vec![CapabilitySpec::Switch {
                id: "led".into(),
                label: "L".into(),
                gpio: 99,
            }],
        };
        assert!(generate(&spec).unwrap_err().contains("out of range"));
    }

    #[test]
    fn bad_firmware_version_format_rejects() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: Some("v1.0".into()),
            capabilities: vec![],
        };
        assert!(generate(&spec).unwrap_err().contains("firmware_version"));
    }

    #[test]
    fn empty_device_name_rejects() {
        let spec = SketchSpec {
            device_name: "".into(),
            firmware_version: None,
            capabilities: vec![],
        };
        assert!(generate(&spec).unwrap_err().contains("device_name"));
    }

    #[test]
    fn slider_floats_format_without_trailing_zero() {
        let spec = SketchSpec {
            device_name: "X".into(),
            firmware_version: None,
            capabilities: vec![
                CapabilitySpec::Slider {
                    id: "a".into(),
                    label: "A".into(),
                    gpio: 5,
                    min: 0.0,
                    max: 100.0,
                },
                CapabilitySpec::Slider {
                    id: "b".into(),
                    label: "B".into(),
                    gpio: 6,
                    min: 0.5,
                    max: 1.5,
                },
            ],
        };
        let out = generate(&spec).unwrap();
        assert!(out.contains("trellis.addSlider(\"a\", \"A\", 0, 100, 5);"));
        assert!(out.contains("trellis.addSlider(\"b\", \"B\", 0.5, 1.5, 6);"));
    }

    /// Hardware-test GATE helper: writes a representative all-kinds sketch to
    /// `/tmp/trellis_sketch_gen_smoke/all_kinds/all_kinds.ino` so that
    /// `arduino-cli compile` can verify the template tracks the real Trellis
    /// library API surface. Marked `#[ignore]` because it's a one-shot smoke
    /// check, not a regression test — invoke explicitly with
    /// `cargo test --lib emit_smoke_fixture -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn emit_smoke_fixture() {
        let spec = SketchSpec {
            device_name: "Smoke Demo".into(),
            firmware_version: Some("1.0.0".into()),
            capabilities: vec![
                CapabilitySpec::Switch {
                    id: "led".into(),
                    label: "LED".into(),
                    gpio: 2,
                },
                CapabilitySpec::Slider {
                    id: "dim".into(),
                    label: "Dim".into(),
                    gpio: 5,
                    min: 0.0,
                    max: 100.0,
                },
                CapabilitySpec::Color {
                    id: "rgb".into(),
                    label: "RGB".into(),
                },
                CapabilitySpec::Sensor {
                    id: "temp".into(),
                    label: "Temperature".into(),
                    unit: "C".into(),
                },
            ],
        };
        let out = generate(&spec).unwrap();
        let dir = std::path::PathBuf::from("/tmp/trellis_sketch_gen_smoke/all_kinds");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("all_kinds.ino");
        std::fs::write(&path, &out).unwrap();
        eprintln!("wrote smoke fixture to {}", path.display());
    }

    #[test]
    fn deserialize_from_json_with_kind_tag() {
        let json = r#"{
            "device_name": "JsonTest",
            "firmware_version": "1.2.3",
            "capabilities": [
                {"kind": "switch", "id": "led", "label": "LED", "gpio": 2},
                {"kind": "color", "id": "rgb", "label": "RGB"}
            ]
        }"#;
        let spec: SketchSpec = serde_json::from_str(json).unwrap();
        let out = generate(&spec).unwrap();
        assert!(out.contains("trellis.addSwitch(\"led\", \"LED\", 2);"));
        assert!(out.contains("trellis.addColor(\"rgb\", \"RGB\");"));
    }
}
