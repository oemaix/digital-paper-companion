//! Session authentication (protocol §5; FR-REG-4).
//!
//! Flow: `GET /auth/nonce/{client_id}` → sign the *Base64 text* of the nonce
//! (not its decoded bytes) with RSA-PKCS#1-v1.5/SHA-256 → `PUT /auth` →
//! extract the `Credentials` cookie (the device's `Set-Cookie` is not fully
//! RFC-compliant; parse manually, protocol §10.5).

// TODO(FR-REG-4): authenticate(http, addr, credentials) -> SessionCookie
