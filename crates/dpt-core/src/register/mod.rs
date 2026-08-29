//! One-time registration (pairing) handshake (protocol §4; FR-REG-1/2/3).
//!
//! Six-message authenticated key exchange over plain HTTP on port 8080:
//! Diffie-Hellman (RFC 3526 group 14) → PBKDF2 key derivation → PIN-based
//! mutual authentication → HMAC-chained messages → AES-128-CBC key wrap.
//!
//! Design: the message/crypto logic in [`flow`] is a pure state machine
//! (bytes in, bytes out) so it can be unit-tested against fixtures without
//! any network I/O (NFR-QLT-2).
//!
//! Critical pitfalls (protocol §10): hash the device DH public key `yb`
//! exactly as received (256 *or* 257 bytes); encode the client key `ya` as
//! 257 bytes (leading 0x00); the key-wrap IV is *appended* to the ciphertext.

pub mod dh;
pub mod flow;
pub mod kdf;
pub mod wrap;
