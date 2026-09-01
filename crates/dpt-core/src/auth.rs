//! Session authentication (protocol §5; FR-REG-4).
//!
//! Flow: `GET /auth/nonce/{client_id}` → sign the *Base64 text* of the nonce
//! (not its decoded bytes) with RSA-PKCS#1-v1.5/SHA-256 → `PUT /auth` →
//! extract the `Credentials` cookie (the device's `Set-Cookie` is not fully
//! RFC-compliant; parse manually, protocol §10.5).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use sha2::Sha256;

use crate::Error;

/// Signs the nonce string with the client's RSA private key and returns the
/// Base64-encoded signature. The ASCII bytes of the nonce *string* are
/// signed, exactly as received (protocol §5 step 2).
pub fn sign_nonce(private_key_pem: &str, nonce: &str) -> Result<String, Error> {
    let key = RsaPrivateKey::from_pkcs8_pem(private_key_pem)
        .map_err(|e| Error::Auth(format!("cannot load private key: {e}")))?;
    let signing_key = SigningKey::<Sha256>::new(key);
    let signature = signing_key
        .try_sign(nonce.as_bytes())
        .map_err(|e| Error::Auth(format!("signing failed: {e}")))?;
    Ok(B64.encode(signature.to_bytes()))
}

/// Parses an RSA private key in PKCS#8 *or* PKCS#1 PEM form and returns it
/// normalized to PKCS#8 PEM — the form the rest of this crate expects.
/// Sony's app and `dptrp1` store `privatekey.dat` as PKCS#1
/// ("BEGIN RSA PRIVATE KEY"), so credential import (FR-REG-6) needs this.
pub fn normalize_private_key_pem(pem: &str) -> Result<String, Error> {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::EncodePrivateKey;

    let key = RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .map_err(|e| Error::Auth(format!("cannot parse private key: {e}")))?;
    key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .map(|p| p.to_string())
        .map_err(|e| Error::Auth(format!("cannot re-encode private key: {e}")))
}

/// Extracts the `Credentials` cookie value from a `Set-Cookie` header value,
/// tolerating the device's non-RFC formatting: take the part before the
/// first `; `, split on the first `=`, and use the remainder.
pub fn extract_credentials_cookie(set_cookie: &str) -> Option<String> {
    let first = set_cookie.split("; ").next()?;
    let (name, value) = first.split_once('=')?;
    if name.trim() == "Credentials" {
        Some(value.trim().to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PKCS#1 test key like Sony's app / `dptrp1` write to
    /// `privatekey.dat`. Test fixture only — not a real credential.
    const PKCS1_TEST_KEY: &str = "-----BEGIN RSA PRIVATE KEY-----
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

    #[test]
    fn normalizes_pkcs1_to_pkcs8() {
        let pkcs8 = normalize_private_key_pem(PKCS1_TEST_KEY).unwrap();
        assert!(pkcs8.starts_with("-----BEGIN PRIVATE KEY-----"));
        // Idempotent: PKCS#8 input passes through re-encoded.
        let again = normalize_private_key_pem(&pkcs8).unwrap();
        assert_eq!(pkcs8, again);
        // The normalized key must be usable for signing.
        sign_nonce(&pkcs8, "dGVzdC1ub25jZQ==").unwrap();
    }

    #[test]
    fn rejects_garbage_key() {
        assert!(normalize_private_key_pem("not a key").is_err());
    }

    #[test]
    fn parses_non_rfc_set_cookie() {
        let hdr = "Credentials=abc.def-123; Path=/; HttpOnly";
        assert_eq!(
            extract_credentials_cookie(hdr).as_deref(),
            Some("abc.def-123")
        );
        assert_eq!(extract_credentials_cookie("Other=1; Path=/"), None);
    }
}
