//! Planning phase (docs/06 §5): pure, deterministic decision function.
//!
//! `plan(check, local, remote, mode) -> Vec<Action>` implementing the
//! two-way decision table (§5.1), conflict policy "newer wins, loser kept"
//! (§5.2), first-run matrix (§5.3), folder rules (§5.4), mirror modes
//! (§5.5) and the mass-deletion guard threshold (§5.6).

// TODO: Action enum { Upload, Download, DeleteLocal, DeleteRemote,
// CreateLocalDir, CreateRemoteDir, ConflictResolve { winner } } and plan().
