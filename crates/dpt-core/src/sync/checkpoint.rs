//! Checkpoint persistence (docs/06 §7).
//!
//! One schema-versioned JSON file per sync pair, written atomically
//! (write-temp-then-rename, NFR-REL-1). A corrupt checkpoint is backed up
//! and treated as "no checkpoint" (safe first-run semantics).

// TODO: Checkpoint { version, pair_id, device_serial, entries } +
// load/save with unknown-field preservation.
