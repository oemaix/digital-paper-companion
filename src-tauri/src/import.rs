//! Credential import from Sony's Digital Paper App or `dptrp1`
//! (FR-REG-6; docs/07 §2).
//!
//! Both store the pairing output as two files: `deviceid.dat` (the client
//! id, plain text) and `privatekey.dat` (the RSA private key, PEM — usually
//! PKCS#1). The default locations are scanned recursively because Sony's
//! app may nest the files in per-device subfolders; a manual file picker in
//! the UI is the fallback. Source files are never modified.

use std::path::{Path, PathBuf};

use serde::Serialize;

use dpt_core::model::Credentials;

use crate::error::AppError;

const DEVICEID_FILE: &str = "deviceid.dat";
const PRIVATEKEY_FILE: &str = "privatekey.dat";
const MAX_SCAN_DEPTH: u32 = 4;

/// A `deviceid.dat`/`privatekey.dat` pair found in a default location.
#[derive(Debug, Clone, Serialize)]
pub struct ImportCandidate {
    pub deviceid_path: String,
    pub privatekey_path: String,
    /// `"sony"` (official Digital Paper App) or `"dptrp1"` (dpt-rp1-py).
    pub origin: String,
}

/// Scans the platform-default locations (docs/07 §2) for credential pairs.
pub fn find_candidates() -> Vec<ImportCandidate> {
    let mut roots: Vec<(PathBuf, &str)> = Vec::new();

    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            roots.push((
                PathBuf::from(appdata)
                    .join("Sony Corporation")
                    .join("Digital Paper App"),
                "sony",
            ));
        }
    }
    if let Some(home) = home_dir() {
        if cfg!(target_os = "macos") {
            roots.push((
                home.join("Library/Application Support/Sony Corporation/Digital Paper App"),
                "sony",
            ));
        }
        // dpt-rp1-py uses ~/.config/dpt on every platform.
        roots.push((home.join(".config").join("dpt"), "dptrp1"));
    }

    let mut out = Vec::new();
    for (root, origin) in roots {
        scan(&root, origin, 0, &mut out);
    }
    out
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn scan(dir: &Path, origin: &str, depth: u32, out: &mut Vec<ImportCandidate>) {
    if depth > MAX_SCAN_DEPTH {
        return;
    }
    let deviceid = dir.join(DEVICEID_FILE);
    let privatekey = dir.join(PRIVATEKEY_FILE);
    if deviceid.is_file() && privatekey.is_file() {
        out.push(ImportCandidate {
            deviceid_path: deviceid.to_string_lossy().into_owned(),
            privatekey_path: privatekey.to_string_lossy().into_owned(),
            origin: origin.to_string(),
        });
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan(&path, origin, depth + 1, out);
        }
    }
}

/// Reads and validates a credential pair: the client id must be a plausible
/// identifier and the key must parse (it is normalized to PKCS#8, the form
/// `dpt-core` signs with). Authentication against the device happens in the
/// import command; only then are the credentials stored.
pub fn read_credentials(
    deviceid_path: &Path,
    privatekey_path: &Path,
) -> Result<Credentials, AppError> {
    let client_id = std::fs::read_to_string(deviceid_path)
        .map_err(|e| AppError::new("io", format!("cannot read deviceid.dat: {e}")))?
        .trim()
        .to_string();
    if client_id.is_empty() || client_id.len() > 128 || !client_id.is_ascii() {
        return Err(AppError::new(
            "invalid_credentials",
            "deviceid.dat does not contain a client id",
        ));
    }
    let pem = std::fs::read_to_string(privatekey_path)
        .map_err(|e| AppError::new("io", format!("cannot read privatekey.dat: {e}")))?;
    let private_key_pem = dpt_core::auth::normalize_private_key_pem(&pem).map_err(|_| {
        AppError::new(
            "invalid_credentials",
            "privatekey.dat is not a readable RSA private key",
        )
    })?;
    Ok(Credentials {
        client_id,
        private_key_pem,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
MIICXgIBAAKBgQDOXAwZaFRzBagC7I8hrEdUMc1VF7TDzWuOhZXSh4vmNgRrdKyP
DjOIJmKUpFq1rUfhNd9nqdNynYN/qMirHS0Uh/MkVRBnLWfK0ZGD0pyAncC3rkiz
USRme0KWJl9E2pGkCz2hqCwKwaOhpxHuThEaegkVHJiORKwhThC4B1jXNQIDAQAB
AoGAG5+8yuXpcCBQtlt+aY6LWdz01LBAtXlZLZH6VV1pv955Rv0uYFQRV+dziNxb
fDh/B8nTZygXsx8czEkG28kjEH/wx23/MzzqzcgGWu6JzUVATx14LiiCnlUvZRjk
EBnIn2zz2Ig+ruOT4nYxZdUddNI2DSqVNJMfpxH26o5iox0CQQDvJPTrfomCg6gX
YsZ93yb7Mut8oxL77d7/aPm76pTwUjFGH19GsUsGFtHZ0k36cfsV2o1ffmkZLdIP
BRecetH/AkEA3OeIPoKgeQVDpdb+yS1oml3QdfMj8hfz5RUo+D0oM4cq8K06tCQy
JDoVgvWQyaXk89618SYxLsFF4/lBxseuywJBAKMaV5kOEodbeBeLHMnYmuOU1RuK
tXXxxLf6Rumtkqtdw5GJ8Bds8DhU9AdV8i0v9Anxp55Lvy6XG792v6XP9s0CQQCF
nEjMvkd/S07SRMqQNbXaADowzSIFsLUk7vp7wsnI+M1hCvXBtU7amIMgVZUAUiW7
1w2m0NnYlK/IJp/BMk+nAkEAxzop0tX+yOZ9y0H+5hQBYaRg9tDuYV2xcdd8sDsU
jNfJePOAHkMpR5X+0QjccaYH0LyvuQlQCzZb2p+p2/7/wA==
-----END RSA PRIVATE KEY-----
";

    fn write(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, contents).unwrap();
        p
    }

    #[test]
    fn scan_finds_nested_pairs() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("device-A");
        std::fs::create_dir_all(&nested).unwrap();
        write(
            &nested,
            DEVICEID_FILE,
            "6fa4b2c1-0000-4000-8000-000000000000\n",
        );
        write(&nested, PRIVATEKEY_FILE, TEST_KEY);
        // A directory with only one of the two files is not a candidate.
        let partial = tmp.path().join("partial");
        std::fs::create_dir_all(&partial).unwrap();
        write(&partial, DEVICEID_FILE, "id");

        let mut out = Vec::new();
        scan(tmp.path(), "sony", 0, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].deviceid_path.ends_with(DEVICEID_FILE));
        assert_eq!(out[0].origin, "sony");
    }

    #[test]
    fn read_credentials_normalizes_key() {
        let tmp = tempfile::tempdir().unwrap();
        let id = write(
            tmp.path(),
            DEVICEID_FILE,
            "6fa4b2c1-0000-4000-8000-000000000000\n",
        );
        let key = write(tmp.path(), PRIVATEKEY_FILE, TEST_KEY);
        let creds = read_credentials(&id, &key).unwrap();
        assert_eq!(creds.client_id, "6fa4b2c1-0000-4000-8000-000000000000");
        assert!(creds
            .private_key_pem
            .starts_with("-----BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn read_credentials_rejects_bad_input() {
        let tmp = tempfile::tempdir().unwrap();
        let id = write(tmp.path(), DEVICEID_FILE, "");
        let key = write(tmp.path(), PRIVATEKEY_FILE, TEST_KEY);
        assert!(read_credentials(&id, &key).is_err());

        let id = write(tmp.path(), DEVICEID_FILE, "valid-id");
        let key = write(tmp.path(), PRIVATEKEY_FILE, "not a key");
        assert!(read_credentials(&id, &key).is_err());
    }
}
