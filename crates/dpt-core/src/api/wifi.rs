//! Wi-Fi management (protocol §7.6; FR-SET-4).
//!
//! SSIDs are Base64-encoded (UTF-8 bytes) in list/register payloads.
//! Passphrases are forwarded to the device and never persisted (NFR-SEC-5).

// TODO: radio on/off, list stored networks, scan, register network, remove.
