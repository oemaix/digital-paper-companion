//! Snapshot phase (docs/06 §3): build the local, remote and checkpoint
//! views as maps `relpath → state`, with NFC path normalization and the
//! platform rules of docs/06 §3.1.

// TODO: LocalTree/RemoteTree/CheckView builders; NFC normalization;
// Windows case-insensitive matching; reversible illegal-char escaping.
