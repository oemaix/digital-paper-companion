//! Credential storage (docs/07 §2; NFR-SEC-1).
//!
//! Primary store: the OS keychain via the `keyring` crate (Windows
//! Credential Manager / macOS Keychain / Linux Secret Service). Fallback,
//! used when no Secret Service is available (e.g. headless Linux): an
//! encrypted file (ChaCha20-Poly1305) under the config dir, with the key in
//! a sibling `0600` file. Credentials are never written in plain text and
//! never logged.

use std::path::PathBuf;

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use dpt_core::model::Credentials;

use crate::error::AppError;

const SERVICE: &str = "digital-paper-companion";

#[derive(Serialize, Deserialize)]
struct StoredCreds {
    client_id: String,
    private_key_pem: String,
}

impl From<&Credentials> for StoredCreds {
    fn from(c: &Credentials) -> Self {
        Self {
            client_id: c.client_id.clone(),
            private_key_pem: c.private_key_pem.clone(),
        }
    }
}

impl From<StoredCreds> for Credentials {
    fn from(s: StoredCreds) -> Self {
        Credentials {
            client_id: s.client_id,
            private_key_pem: s.private_key_pem,
        }
    }
}

/// Stores per-device credentials, keyed by device serial.
#[derive(Clone)]
pub struct CredentialStore {
    fallback_dir: PathBuf,
}

impl CredentialStore {
    pub fn new(fallback_dir: PathBuf) -> Self {
        Self { fallback_dir }
    }

    fn account(serial: &str) -> String {
        format!("dpt:{serial}")
    }

    pub fn save(&self, serial: &str, creds: &Credentials) -> Result<(), AppError> {
        let json = serde_json::to_string(&StoredCreds::from(creds))?;
        match keyring::Entry::new(SERVICE, &Self::account(serial)) {
            Ok(entry) if entry.set_password(&json).is_ok() => Ok(()),
            _ => {
                tracing::warn!("OS keychain unavailable; using encrypted file fallback");
                self.save_fallback(serial, json.as_bytes())
            }
        }
    }

    pub fn load(&self, serial: &str) -> Result<Option<Credentials>, AppError> {
        if let Ok(entry) = keyring::Entry::new(SERVICE, &Self::account(serial)) {
            match entry.get_password() {
                Ok(json) => {
                    let stored: StoredCreds = serde_json::from_str(&json)?;
                    return Ok(Some(stored.into()));
                }
                Err(keyring::Error::NoEntry) => {}
                Err(_) => { /* fall through to fallback */ }
            }
        }
        match self.load_fallback(serial)? {
            Some(bytes) => {
                let stored: StoredCreds = serde_json::from_slice(&bytes)?;
                Ok(Some(stored.into()))
            }
            None => Ok(None),
        }
    }

    pub fn delete(&self, serial: &str) -> Result<(), AppError> {
        if let Ok(entry) = keyring::Entry::new(SERVICE, &Self::account(serial)) {
            let _ = entry.delete_credential();
        }
        let _ = std::fs::remove_file(self.enc_path(serial));
        Ok(())
    }

    // ---- encrypted-file fallback ----------------------------------------

    fn enc_path(&self, serial: &str) -> PathBuf {
        self.fallback_dir.join(format!("{serial}.enc"))
    }
    fn key_path(&self) -> PathBuf {
        self.fallback_dir.join(".key")
    }

    fn cipher(&self) -> Result<ChaCha20Poly1305, AppError> {
        std::fs::create_dir_all(&self.fallback_dir)?;
        let key_path = self.key_path();
        let key_bytes = match std::fs::read(&key_path) {
            Ok(b) if b.len() == 32 => b,
            _ => {
                let mut k = vec![0u8; 32];
                rand::thread_rng().fill_bytes(&mut k);
                std::fs::write(&key_path, &k)?;
                restrict_permissions(&key_path);
                k
            }
        };
        Ok(ChaCha20Poly1305::new(Key::from_slice(&key_bytes)))
    }

    fn save_fallback(&self, serial: &str, plaintext: &[u8]) -> Result<(), AppError> {
        let cipher = self.cipher()?;
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|_| AppError::new("crypto", "credential encryption failed"))?;
        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&ciphertext);
        std::fs::write(self.enc_path(serial), &blob)?;
        restrict_permissions(&self.enc_path(serial));
        Ok(())
    }

    fn load_fallback(&self, serial: &str) -> Result<Option<Vec<u8>>, AppError> {
        let blob = match std::fs::read(self.enc_path(serial)) {
            Ok(b) => b,
            Err(_) => return Ok(None),
        };
        if blob.len() < 12 {
            return Err(AppError::new("crypto", "corrupt credential file"));
        }
        let (nonce_bytes, ciphertext) = blob.split_at(12);
        let cipher = self.cipher()?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
            .map_err(|_| AppError::new("crypto", "credential decryption failed"))?;
        Ok(Some(plaintext))
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}
