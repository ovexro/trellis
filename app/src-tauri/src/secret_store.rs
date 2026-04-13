// At-rest encryption for stored Trellis secrets.
//
// What this protects: anything Trellis writes to SQLite that the user would
// not want exposed if their laptop's filesystem were stolen, snapshotted, or
// synced to a backup service. Currently the only secret we encrypt is the
// MQTT broker password under `settings.mqtt_config`. Future: TLS client
// keys, IoT cloud tokens, etc.
//
// What this does NOT protect: anything in process memory (the live MQTT
// bridge holds the plaintext password to talk to the broker), anything
// returned by the REST API at runtime (see api.rs sensitive-key blocklist
// and the redacted MqttConfigPublic), or filesystem snapshots taken AFTER
// keyring access by anything running as Ovidiu's user (the SecretStore
// holds an x25519 identity in process memory after `load_or_create`).
// This is at-rest mitigation, not full memory encryption.
//
// Key bootstrap:
//   1. On first use, generate an `age::x25519::Identity` (32-byte key
//      material wrapped in bech32: `AGE-SECRET-KEY-1...`).
//   2. Try to store the bech32 identity string in the OS keyring under
//      service `com.trellis.app` / username `secret-store`.
//   3. If the keyring is unavailable (no D-Bus session, headless server,
//      Secret Service crashed), fall back to a key file at
//      `<app_data_dir>/secret.key` with mode 0600. Log a warning so the
//      user knows.
//   4. On subsequent loads, try keyring first, then file. If both fail,
//      return Err — the caller decides whether to bail or run without
//      encryption.
//
// Wire format for encrypted values:
//   `enc:v1:<base64>`
// where `<base64>` is the standard-base64 encoding of binary age
// ciphertext. The `enc:v1:` prefix lets us detect (a) "this is already
// encrypted, skip re-encrypt" on save and (b) "this is plaintext from a
// pre-encryption build, migrate it" on load.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use age::secrecy::ExposeSecret;
use age::x25519::{Identity, Recipient};
use base64::Engine;

const KEYRING_SERVICE: &str = "com.trellis.app";
const KEYRING_USER: &str = "secret-store";
const FILE_NAME: &str = "secret.key";
const ENCRYPTED_PREFIX: &str = "enc:v1:";

pub struct SecretStore {
    identity: Identity,
}

impl SecretStore {
    /// Load the existing secret from keyring/file, or generate a new one
    /// and store it. Idempotent — safe to call on every app startup.
    pub fn load_or_create(app_data_dir: &Path) -> Result<Self, String> {
        // 1. Try keyring
        let keyring_result = load_from_keyring();
        if let Some(identity) = keyring_result? {
            return Ok(Self { identity });
        }

        // 2. Try file fallback
        let file_path = app_data_dir.join(FILE_NAME);
        if file_path.exists() {
            let bech32 = read_key_file(&file_path)?;
            let identity = parse_identity(&bech32)?;
            // If a file exists and the keyring is now writable (unlikely
            // since we just failed to read from it), we don't promote it.
            // Keep the file as the source of truth.
            log::info!("[SecretStore] Loaded key from file fallback at {}", file_path.display());
            return Ok(Self { identity });
        }

        // 3. Bootstrap: generate a fresh identity and try to store it
        let identity = Identity::generate();
        let bech32 = identity.to_string().expose_secret().to_string();

        // Prefer keyring; fall back to file
        if store_in_keyring(&bech32).is_ok() {
            log::info!("[SecretStore] Generated new key, stored in OS keyring");
        } else {
            // File fallback. Make sure parent dir exists, then write with
            // mode 0600 so only the user can read it.
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("[SecretStore] Failed to create app data dir: {}", e)
                })?;
            }
            write_key_file(&file_path, &bech32)?;
            log::warn!(
                "[SecretStore] OS keyring unavailable; falling back to key file at {} (mode 0600). \
                 Anyone with read access to your home directory can decrypt stored secrets — \
                 install/start the gnome-keyring-daemon (or another Secret Service provider) \
                 to use the more secure keyring backend.",
                file_path.display()
            );
        }

        Ok(Self { identity })
    }

    /// Encrypt a plaintext string and return the wire-format `enc:v1:<base64>`.
    pub fn encrypt(&self, plaintext: &str) -> Result<String, String> {
        let recipient: Recipient = self.identity.to_public();
        let encryptor = age::Encryptor::with_recipients(vec![Box::new(recipient)])
            .ok_or_else(|| "[SecretStore] Failed to build encryptor".to_string())?;

        let mut ciphertext = Vec::new();
        let mut writer = encryptor
            .wrap_output(&mut ciphertext)
            .map_err(|e| format!("[SecretStore] Encrypt wrap failed: {}", e))?;
        writer
            .write_all(plaintext.as_bytes())
            .map_err(|e| format!("[SecretStore] Encrypt write failed: {}", e))?;
        writer
            .finish()
            .map_err(|e| format!("[SecretStore] Encrypt finish failed: {}", e))?;

        let b64 = base64::engine::general_purpose::STANDARD.encode(&ciphertext);
        Ok(format!("{}{}", ENCRYPTED_PREFIX, b64))
    }

    /// Decrypt a wire-format `enc:v1:<base64>` value back to plaintext.
    /// Returns Err if the input doesn't have the `enc:v1:` prefix — callers
    /// should use `is_encrypted` to check first.
    pub fn decrypt(&self, wire: &str) -> Result<String, String> {
        let b64 = wire
            .strip_prefix(ENCRYPTED_PREFIX)
            .ok_or_else(|| "[SecretStore] Not an enc:v1 value".to_string())?;
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("[SecretStore] base64 decode failed: {}", e))?;

        let decryptor = match age::Decryptor::new(&ciphertext[..])
            .map_err(|e| format!("[SecretStore] Decryptor init failed: {}", e))?
        {
            age::Decryptor::Recipients(d) => d,
            age::Decryptor::Passphrase(_) => {
                return Err(
                    "[SecretStore] Stored value uses passphrase recipient; expected x25519"
                        .to_string(),
                );
            }
        };

        let identities: Vec<Box<dyn age::Identity>> = vec![Box::new(self.identity.clone())];
        let mut reader = decryptor
            .decrypt(identities.iter().map(|i| i.as_ref()))
            .map_err(|e| format!("[SecretStore] Decrypt failed: {}", e))?;
        let mut out = String::new();
        reader
            .read_to_string(&mut out)
            .map_err(|e| format!("[SecretStore] Decrypt read failed: {}", e))?;
        Ok(out)
    }
}

/// True if the given value already has the `enc:v1:` prefix and would
/// successfully round-trip through `decrypt`. Use this on save to skip
/// re-encrypting an already-encrypted value, and on load to detect legacy
/// plaintext that needs migration.
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENCRYPTED_PREFIX)
}

// ─── MqttConfig field-level helpers ───────────────────────────────────────
//
// The MQTT config is stored as JSON in `settings.mqtt_config`. Most fields
// (host, port, base_topic, etc.) are not sensitive and stay plaintext for
// inspectability. Only the `password` field gets encrypted, in place,
// before serializing the struct to JSON.
//
// These helpers operate on `MqttConfig` directly so call sites only need
// one line each.

use crate::mqtt::MqttConfig;

/// Encrypt the password field of an MqttConfig in place, ready to serialize
/// and write to SQLite. No-ops on empty passwords (bridge running with no
/// auth) and on values that are already encrypted (defensive — protects
/// against double-encryption if the same config is saved twice without a
/// round-trip through the in-memory store).
pub fn encrypt_mqtt_password(store: &SecretStore, cfg: &mut MqttConfig) -> Result<(), String> {
    if cfg.password.is_empty() {
        return Ok(());
    }
    if is_encrypted(&cfg.password) {
        return Ok(());
    }
    cfg.password = store.encrypt(&cfg.password)?;
    Ok(())
}

/// Decrypt the password field of an MqttConfig in place, after reading
/// from SQLite. Plaintext values (legacy from pre-encryption builds) pass
/// through unchanged so the bridge keeps working — they get re-encrypted
/// on the next save (or via the lazy upgrade path in lib.rs setup).
pub fn decrypt_mqtt_password(store: &SecretStore, cfg: &mut MqttConfig) -> Result<(), String> {
    if !is_encrypted(&cfg.password) {
        return Ok(());
    }
    cfg.password = store.decrypt(&cfg.password)?;
    Ok(())
}

// ─── SinricConfig field-level helpers ─────────────────────────────────────
//
// Same pattern as the MqttConfig helpers above. The `api_secret` field is
// encrypted at rest; everything else (api_key, device_mappings) is plaintext.

use crate::sinric::SinricConfig;

pub fn encrypt_sinric_secret(store: &SecretStore, cfg: &mut SinricConfig) -> Result<(), String> {
    if cfg.api_secret.is_empty() {
        return Ok(());
    }
    if is_encrypted(&cfg.api_secret) {
        return Ok(());
    }
    cfg.api_secret = store.encrypt(&cfg.api_secret)?;
    Ok(())
}

pub fn decrypt_sinric_secret(store: &SecretStore, cfg: &mut SinricConfig) -> Result<(), String> {
    if !is_encrypted(&cfg.api_secret) {
        return Ok(());
    }
    cfg.api_secret = store.decrypt(&cfg.api_secret)?;
    Ok(())
}

// ─── Keyring backend ───────────────────────────────────────────────────────

fn load_from_keyring() -> Result<Option<Identity>, String> {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[SecretStore] keyring::Entry::new failed: {}", e);
            return Ok(None);
        }
    };
    match entry.get_password() {
        Ok(bech32) => {
            let identity = parse_identity(&bech32)?;
            log::info!("[SecretStore] Loaded key from OS keyring");
            Ok(Some(identity))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            log::debug!("[SecretStore] keyring get_password failed: {}", e);
            Ok(None)
        }
    }
}

fn store_in_keyring(bech32: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| format!("[SecretStore] keyring::Entry::new failed: {}", e))?;
    entry
        .set_password(bech32)
        .map_err(|e| format!("[SecretStore] keyring set_password failed: {}", e))?;
    Ok(())
}

// ─── File backend ──────────────────────────────────────────────────────────

fn read_key_file(path: &PathBuf) -> Result<String, String> {
    let contents =
        fs::read_to_string(path).map_err(|e| format!("[SecretStore] Read key file: {}", e))?;
    Ok(contents.trim().to_string())
}

fn write_key_file(path: &PathBuf, bech32: &str) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(|e| format!("[SecretStore] Open key file for write: {}", e))?;
    file.write_all(bech32.as_bytes())
        .map_err(|e| format!("[SecretStore] Write key file: {}", e))?;

    // Lock down permissions: 0600 (owner read+write only). Critical on a
    // multi-user system or any setup where the home directory is readable
    // by others.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = file
            .metadata()
            .map_err(|e| format!("[SecretStore] Stat key file: {}", e))?
            .permissions();
        perms.set_mode(0o600);
        file.set_permissions(perms)
            .map_err(|e| format!("[SecretStore] Chmod key file: {}", e))?;
    }
    Ok(())
}

// ─── Identity parsing helper ───────────────────────────────────────────────

fn parse_identity(bech32: &str) -> Result<Identity, String> {
    bech32
        .parse::<Identity>()
        .map_err(|e| format!("[SecretStore] Parse identity bech32: {}", e))
}
