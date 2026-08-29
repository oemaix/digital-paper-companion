//! The six-message handshake state machine M1…M6 (protocol §4.3, §4.7).
//!
//! Pure (no I/O): each step consumes the previous device message and
//! produces the next client message. Typed phase structs make illegal
//! transitions unrepresentable, and every step is testable against recorded
//! fixtures. The HTTP transport lives in the parent module.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::RngCore;
use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::RsaPrivateKey;
use serde::{Deserialize, Serialize};

use super::dh::DhKeypair;
use super::kdf::{self, hmac_sha256, DerivedKeys};
use super::wrap::{unwrap, wrap};
use crate::model::{Credentials, Registration};
use crate::Error;

// ---- Wire messages (flat JSON, single-letter keys, base64 values) --------

/// M1: device → client (response to `POST /register/pin`).
#[derive(Debug, Deserialize)]
pub struct M1 {
    /// `n1` — device nonce.
    pub a: String,
    /// `mac` — opaque device identifier.
    pub b: String,
    /// `yb` — device DH public key (raw bytes, may be 256 or 257 long).
    pub c: String,
}

/// M2: client → device (body of `POST /register/hash`).
#[derive(Debug, Serialize)]
pub struct M2 {
    pub a: String,
    pub b: String,
    pub c: String,
    pub d: String,
    pub e: String,
}

/// M3: device → client (response to `POST /register/hash`).
#[derive(Debug, Deserialize)]
pub struct M3 {
    pub a: String,
    pub b: String,
    pub e: String,
}

/// M4: client → device (body of `POST /register/ca`).
#[derive(Debug, Serialize)]
pub struct M4 {
    pub a: String,
    pub b: String,
    pub d: String,
    pub e: String,
}

/// M5: device → client (response to `POST /register/ca`).
#[derive(Debug, Deserialize)]
pub struct M5 {
    pub a: String,
    pub d: String,
    pub e: String,
}

/// M6: client → device (body of `POST /register`).
#[derive(Debug, Serialize)]
pub struct M6 {
    pub a: String,
    pub d: String,
    pub e: String,
}

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    let mut v = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

fn b64d(s: &str) -> Result<Vec<u8>, Error> {
    B64.decode(s.trim())
        .map_err(|e| Error::Registration(format!("invalid base64 in device message: {e}")))
}

// ---- Phase 1: after M1, awaiting M3 --------------------------------------

/// Handshake state after producing M2, before M3 arrives.
pub struct Started {
    keys: DerivedKeys,
    n1: Vec<u8>,
    n2: Vec<u8>,
    mac: Vec<u8>,
    yb: Vec<u8>,
    ya: Vec<u8>,
    m2hmac: Vec<u8>,
}

/// Begins the handshake: consumes M1, derives keys and produces M2.
pub fn start(m1: &M1) -> Result<(Started, M2), Error> {
    let n1 = b64d(&m1.a)?;
    let mac = b64d(&m1.b)?;
    let yb = b64d(&m1.c)?;

    let dh = DhKeypair::generate();
    let ya = dh.encode_public();
    let zz = dh.shared_secret(&yb)?;

    let mut n2 = vec![0u8; 16];
    rand::thread_rng().fill_bytes(&mut n2);

    let keys = kdf::derive(&zz, &n1, &mac, &n2);

    // m2hmac = HMAC(authKey, n1 ‖ mac ‖ yb ‖ n1 ‖ n2 ‖ mac ‖ ya)
    let m2hmac = hmac_sha256(
        &keys.auth_key,
        &concat(&[&n1, &mac, &yb, &n1, &n2, &mac, &ya]),
    )
    .to_vec();

    let m2 = M2 {
        a: B64.encode(&n1),
        b: B64.encode(&n2),
        c: B64.encode(&mac),
        d: B64.encode(&ya),
        e: B64.encode(&m2hmac),
    };
    Ok((
        Started {
            keys,
            n1,
            n2,
            mac,
            yb,
            ya,
            m2hmac,
        },
        m2,
    ))
}

impl Started {
    /// Verifies M3 and transitions to awaiting the user's PIN.
    pub fn on_m3(self, m3: &M3) -> Result<AwaitingPin, Error> {
        let n2_echo = b64d(&m3.a)?;
        if n2_echo != self.n2 {
            return Err(Error::Registration("M3 nonce mismatch".into()));
        }
        let e_hash = b64d(&m3.b)?;
        let m3hmac = b64d(&m3.e)?;

        // verify HMAC(authKey, n1 ‖ n2 ‖ mac ‖ ya ‖ m2hmac ‖ n2 ‖ eHash)
        let expected = hmac_sha256(
            &self.keys.auth_key,
            &concat(&[
                &self.n1,
                &self.n2,
                &self.mac,
                &self.ya,
                &self.m2hmac,
                &self.n2,
                &e_hash,
            ]),
        );
        if m3hmac != expected {
            return Err(Error::Registration("M3 HMAC verification failed".into()));
        }

        let _ = &self.mac; // mac is not used past M2
        Ok(AwaitingPin {
            keys: self.keys,
            n1: self.n1,
            n2: self.n2,
            yb: self.yb,
            ya: self.ya,
            e_hash,
            m3hmac,
        })
    }
}

// ---- Phase 2: after M3, awaiting the PIN then M5 -------------------------

/// Handshake state after M3 is verified; the user must supply the PIN.
pub struct AwaitingPin {
    keys: DerivedKeys,
    n1: Vec<u8>,
    n2: Vec<u8>,
    yb: Vec<u8>,
    ya: Vec<u8>,
    e_hash: Vec<u8>,
    m3hmac: Vec<u8>,
}

impl AwaitingPin {
    /// Builds M4 from the PIN displayed on the device.
    pub fn build_m4(self, pin: &str) -> Result<(AwaitingM5, M4), Error> {
        let psk = hmac_sha256(&self.keys.auth_key, pin.as_bytes()).to_vec();

        let mut rs = vec![0u8; 16];
        rand::thread_rng().fill_bytes(&mut rs);

        // rHash = HMAC(authKey, rs ‖ psk ‖ yb ‖ ya)
        let r_hash = hmac_sha256(
            &self.keys.auth_key,
            &concat(&[&rs, &psk, &self.yb, &self.ya]),
        )
        .to_vec();

        let wrapped_rs = wrap(&self.keys.auth_key, key16(&self.keys.key_wrap_key), &rs);

        // m4hmac = HMAC(authKey, n2 ‖ eHash ‖ m3hmac ‖ n1 ‖ rHash ‖ wrappedRs)
        let m4hmac = hmac_sha256(
            &self.keys.auth_key,
            &concat(&[
                &self.n2,
                &self.e_hash,
                &self.m3hmac,
                &self.n1,
                &r_hash,
                &wrapped_rs,
            ]),
        )
        .to_vec();

        let m4 = M4 {
            a: B64.encode(&self.n1),
            b: B64.encode(&r_hash),
            d: B64.encode(&wrapped_rs),
            e: B64.encode(&m4hmac),
        };
        Ok((
            AwaitingM5 {
                keys: self.keys,
                n1: self.n1,
                n2: self.n2,
                yb: self.yb,
                ya: self.ya,
                e_hash: self.e_hash,
                psk,
                r_hash,
                wrapped_rs,
                m4hmac,
            },
            m4,
        ))
    }
}

// ---- Phase 3: after M4, awaiting M5 -------------------------------------

/// Handshake state after M4 is sent; consumes M5 and produces M6.
pub struct AwaitingM5 {
    keys: DerivedKeys,
    n1: Vec<u8>,
    n2: Vec<u8>,
    yb: Vec<u8>,
    ya: Vec<u8>,
    e_hash: Vec<u8>,
    psk: Vec<u8>,
    r_hash: Vec<u8>,
    wrapped_rs: Vec<u8>,
    m4hmac: Vec<u8>,
}

impl AwaitingM5 {
    /// Verifies M5, checks the device's PIN knowledge, generates the
    /// long-term RSA identity, and produces both M6 and the credentials to
    /// persist.
    pub fn on_m5(self, m5: &M5) -> Result<(Registration, M6), Error> {
        let n2_echo = b64d(&m5.a)?;
        if n2_echo != self.n2 {
            return Err(Error::Registration("M5 nonce mismatch".into()));
        }
        let wrapped_es_cert = b64d(&m5.d)?;
        let m5hmac = b64d(&m5.e)?;

        // verify HMAC(authKey, n1 ‖ rHash ‖ wrappedRs ‖ m4hmac ‖ n2 ‖ wrappedEsCert)
        let expected = hmac_sha256(
            &self.keys.auth_key,
            &concat(&[
                &self.n1,
                &self.r_hash,
                &self.wrapped_rs,
                &self.m4hmac,
                &self.n2,
                &wrapped_es_cert,
            ]),
        );
        if m5hmac != expected {
            return Err(Error::Registration("M5 HMAC verification failed".into()));
        }

        let es_cert = unwrap(
            &self.keys.auth_key,
            key16(&self.keys.key_wrap_key),
            &wrapped_es_cert,
        )?;
        if es_cert.len() < 16 {
            return Err(Error::Registration("M5 payload too short".into()));
        }
        let (es, cert_bytes) = es_cert.split_at(16);
        let cert_pem = String::from_utf8(cert_bytes.to_vec())
            .map_err(|_| Error::Registration("device certificate is not valid UTF-8".into()))?;

        // Mutual authentication (protocol §4.7): the device proves it knows
        // the same PIN. eHash (from M3) must equal HMAC(authKey, es‖psk‖yb‖ya)
        // where `es` is the device's secret nonce revealed in M5 and `psk`
        // encodes the PIN the user typed. A wrong PIN fails here.
        let e_hash_check =
            hmac_sha256(&self.keys.auth_key, &concat(&[es, &self.psk, &self.yb, &self.ya]));
        if e_hash_check.as_slice() != self.e_hash.as_slice() {
            return Err(Error::Registration(
                "PIN verification failed (device eHash mismatch)".into(),
            ));
        }

        // Generate the long-term client identity.
        let mut rng = rand::thread_rng();
        let rsa = RsaPrivateKey::new(&mut rng, 2048)
            .map_err(|e| Error::Crypto(format!("RSA key generation failed: {e}")))?;
        let private_key_pem = rsa
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| Error::Crypto(format!("PKCS#8 encoding failed: {e}")))?
            .to_string();
        let public_key_pem = rsa
            .to_public_key()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|e| Error::Crypto(format!("SPKI encoding failed: {e}")))?;

        let client_id = uuid::Uuid::new_v4().to_string();

        // wrappedDIDKPUBC = wrap( UTF8(client_id) ‖ keyPubC )
        let mut didk = Vec::new();
        didk.extend_from_slice(client_id.as_bytes());
        didk.extend_from_slice(public_key_pem.as_bytes());
        let wrapped_didk = wrap(&self.keys.auth_key, key16(&self.keys.key_wrap_key), &didk);

        // m6hmac = HMAC(authKey, n2 ‖ wrappedEsCert ‖ m5hmac ‖ n1 ‖ wrappedDIDKPUBC)
        let m6hmac = hmac_sha256(
            &self.keys.auth_key,
            &concat(&[&self.n2, &wrapped_es_cert, &m5hmac, &self.n1, &wrapped_didk]),
        )
        .to_vec();

        let m6 = M6 {
            a: B64.encode(&self.n1),
            d: B64.encode(&wrapped_didk),
            e: B64.encode(&m6hmac),
        };
        let registration = Registration {
            credentials: Credentials {
                client_id,
                private_key_pem,
            },
            device_cert_pem: cert_pem,
        };
        Ok((registration, m6))
    }
}

fn key16(k: &[u8; 16]) -> &[u8; 16] {
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn started_rejects_wrong_m3_nonce() {
        // Build a valid M1 from a simulated device to exercise start().
        let device_dh = DhKeypair::from_private_bytes(&[0x33; 32]);
        let yb = device_dh.encode_public();
        let m1 = M1 {
            a: B64.encode([0xAAu8; 16]),
            b: B64.encode([0xBBu8; 6]),
            c: B64.encode(&yb),
        };
        let (started, _m2) = start(&m1).unwrap();
        // An M3 echoing the wrong nonce must be rejected.
        let bad_m3 = M3 {
            a: B64.encode([0u8; 16]),
            b: B64.encode([0u8; 32]),
            e: B64.encode([0u8; 32]),
        };
        assert!(started.on_m3(&bad_m3).is_err());
    }
}
