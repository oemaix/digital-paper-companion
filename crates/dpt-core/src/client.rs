//! Authenticated HTTPS client for the main API on port 8443
//! (docs/04 §4.1; NFR-SEC-2, NFR-REL-4).
//!
//! Responsibilities:
//! - TLS with **certificate pinning** against the certificate obtained at
//!   registration (custom `rustls` verifier, byte-equal DER comparison);
//!   never plain "verification off".
//! - Session cookie management with transparent one-shot re-authentication
//!   on auth failure.
//! - Timeouts (connect 5 s, request 30 s, transfer stall 60 s) and retry
//!   policy (idempotent GETs: 2 retries with backoff; mutations: none).

// TODO: DeviceClient { probe, register, connect, ping, ... } per docs/04 §4.1.
