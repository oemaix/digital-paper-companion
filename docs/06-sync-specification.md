# 06 — Sync Specification

This document is the authoritative specification of the sync engine. It
extends the informal description in Appendix B of the
[protocol document](sony-digital-paper-protocol.md) into a testable design.

Sync is entirely **client-side**: the device offers no change feed or
transactions, only the entry-listing and transfer endpoints. The engine is a
classic three-way comparison (checkpoint vs. local vs. remote) executed as
snapshot → plan → apply.

## 1. Concepts

- **Sync pair** — configuration unit: `{ id, local_root (absolute path),
  remote_root (device path, default "Document"), mode, schedule, filters,
  deletion_threshold }`.
- **Mode** — `TwoWay` | `MirrorToLocal` (device is source of truth) |
  `MirrorToRemote` (computer is source of truth).
- **Checkpoint** — persisted snapshot of the tree state as of the end of the
  last successful application of actions (per sync pair).
- **Relative path (relpath)** — the identity of a file across all three
  views: path relative to the pair's root, `/`-separated, Unicode **NFC**
  normalized.

Only **PDF documents and folders** participate. Local non-PDF files are
ignored (never uploaded, never deleted). Filters (FR-SYN-8) remove matching
relpaths from *all three* views before planning.

## 2. Preconditions of a run

1. Device connected and authenticated; otherwise the run fails immediately
   with *device unreachable*.
2. The device clock is set from the computer (`/system/configs/datetime`,
   FR-SET-3). Rationale: change classification compares device-side
   `modified_date` values produced by the device clock.
3. `local_root` exists and is a directory. If it is missing, the run
   **aborts with an error** — it is never treated as "user deleted
   everything" (guards against unmounted drives).
4. A per-pair lock ensures at most one run per pair; distinct pairs may not
   run concurrently against the same device either (single global runner),
   to keep device load predictable.

## 3. Snapshot phase

Three views, each a map `relpath → FileState | FolderState`:

| View | Source | FileState fields |
|---|---|---|
| `remote` | `list_all_entries()` (with 1300-entry fallback), restricted to `remote_root` | `entry_id`, `modified_date`, `file_size`, `file_revision` |
| `local` | filesystem walk of `local_root` | `mtime` (UTC), `size` |
| `check` | checkpoint file | for both sides: `remote_modified_date`, `remote_file_revision`, `local_mtime`, `local_size` |

### 3.1 Path normalization rules

- Relpaths are NFC-normalized before map insertion (macOS filesystems return
  NFD; the device uses NFC — NFR-I18N-3).
- Windows: relpaths are matched case-insensitively but stored with original
  case; a case-only rename appears as delete+create and is handled by the
  normal rules.
- Local file/folder names containing characters illegal on the local OS
  (e.g. `:` on Windows) are mapped with a reversible escape (`%3A`-style)
  when downloading; the mapping is recorded in the checkpoint so future runs
  recognize the file.

## 4. Change detection

For each relpath, each side is classified against the checkpoint:

| Side | Unchanged when | Changed when | New when | Deleted when |
|---|---|---|---|---|
| Remote | present and `file_revision` equal to checkpoint (fallback: `modified_date` and `file_size` equal) | present with different revision/date/size | present but absent from checkpoint | absent but present in checkpoint |
| Local | present and (`mtime`, `size`) equal to checkpoint | present with different `mtime` or `size` | present but absent from checkpoint | absent but present in checkpoint |

Notes:

- `file_revision` is the preferred remote change signal (it changes on any
  modification, including annotation strokes that may not change size).
- Local `mtime` granularity varies by filesystem; comparison uses a 2 s
  tolerance window, and `size` inequality always means changed.
- If there is **no checkpoint** (first run), every path present on either
  side is classified *New* on that side (see 5.3 for the first-run matrix).

## 5. Planning phase

`plan(check, local, remote, mode) -> Vec<Action>` — pure and deterministic.
Actions: `Upload`, `Download`, `DeleteLocal`, `DeleteRemote`,
`CreateLocalDir`, `CreateRemoteDir`, `ConflictResolve{winner}`.

### 5.1 Two-way mode decision table (files)

L = local classification, R = remote classification vs. checkpoint:

| L \ R | Unchanged | Changed | New | Deleted |
|---|---|---|---|---|
| **Unchanged** | — | Download | n/a | DeleteLocal |
| **Changed** | Upload | **Conflict** | n/a | **Conflict** (treat remote as loser-absent: upload, keep local) |
| **New** | n/a | n/a | **Conflict** if content differs¹, else adopt | Upload |
| **Deleted** | DeleteRemote | **Conflict** (remote changed after local delete → Download, no delete) | Download | — (both gone: drop from checkpoint) |

¹ Same relpath appearing new on both sides: if sizes match, adopt silently
(record in checkpoint, no transfer); if sizes differ, treat as Conflict.

n/a cells cannot occur (a path can't be simultaneously new on a side and
present in the checkpoint).

### 5.2 Conflict resolution (FR-SYN-6)

Policy **newer-wins-loser-kept**:

1. Compare remote `modified_date` and local `mtime`.
2. The newer side's content becomes the canonical file at the relpath.
3. The older side's content is preserved:
   - if local loses → local file is renamed to
     `name (conflict YYYY-MM-DD HHMM).pdf` before the download;
   - if remote loses → remote content is first downloaded to that conflict
     name locally, then the local file is uploaded.
4. Conflict copies exist **locally only** and are recorded in the checkpoint
   as local-only artifacts excluded from future upload (they are reported in
   the run summary; the user may delete or re-upload them manually).

No version of a file is ever discarded without a surviving copy.

### 5.3 First run (no checkpoint)

- Path exists only locally → Upload. Only remotely → Download.
- Both sides, equal size → adopt without transfer.
- Both sides, different size → Conflict (newer wins, loser kept).

### 5.4 Folders

- Folders needed by planned file actions are created first
  (`CreateRemoteDir` one level at a time per protocol §7.3.6).
- A folder deleted on one side is deleted on the other **only if** the
  engine deletes all its remaining children under the same rules; a folder
  containing an unresolved conflict or a filtered/ignored file is left in
  place (remote) or left in place with its non-PDF content (local).
- Empty folders sync as folders (created on the other side), matching user
  expectation of a mirrored tree.

### 5.5 Mirror modes

- `MirrorToLocal`: plan only `Download`, `CreateLocalDir`, `DeleteLocal`;
  local changes are overwritten (changed local files are conflict-copied
  first — same safety rule).
- `MirrorToRemote`: symmetric (`Upload`, `CreateRemoteDir`, `DeleteRemote`);
  changed remote files are downloaded to a conflict copy before overwrite.

### 5.6 Mass-deletion guard (FR-SYN-5)

If the plan contains more than `deletion_threshold` deletions (default 10)
on either side, an interactive run shows them for confirmation; a scheduled
run pauses and raises `sync:confirmation-required`. The user may approve,
skip deletions, or cancel.

## 6. Apply phase

- Order: create dirs → uploads & downloads (interleaved, concurrency 2,
  device-friendly) → deletions → folder deletions (deepest first).
- Downloads write `*.part` then rename (NFR-REL-2). Uploads use
  create-entry + put-content with ghost-entry cleanup (FR-TRF-10).
- **Checkpoint is updated incrementally**: after each completed action the
  affected relpath's checkpoint record is updated in memory, and the file is
  flushed atomically every N actions and at the end. An interrupted run
  therefore re-plans only the remaining work next time (FR-SYN-9).
- After all actions, a fresh remote listing of `remote_root` refreshes the
  checkpoint's remote fields (cheap consistency pass; catches entries the
  device modified during the run).
- Per-action failures don't abort the run (except connection loss); they are
  collected and reported. The relpath keeps its old checkpoint record, so
  the action is retried next run.

## 7. Checkpoint format

One JSON file per sync pair (see [07-data-and-security.md](07-data-and-security.md)
for location), written atomically, schema-versioned:

```json
{
  "version": 1,
  "pair_id": "9f0c…",
  "device_serial": "5001…",
  "remote_root": "Document",
  "completed_at": "2026-08-29T10:12:03Z",
  "entries": {
    "Papers/attention.pdf": {
      "type": "file",
      "remote": { "entry_id": "…", "modified_date": "2026-08-28T20:11:00Z",
                   "file_revision": "a21ea4b1c368.2.0", "size": 1834122 },
      "local":  { "mtime": "2026-08-28T20:11:02Z", "size": 1834122 },
      "flags":  []            // e.g. ["conflict_copy"], ["name_escaped"]
    },
    "Papers": { "type": "folder" }
  }
}
```

Unknown future fields are preserved on rewrite. A corrupt/unreadable
checkpoint is treated as *no checkpoint* (first-run semantics — safe because
first-run rules never delete anything) after backing the corrupt file up.

## 8. Scheduling (FR-SYN-4)

- Triggers per pair: `on_connect` (runs once each time the device transitions
  to connected, after a 10 s settle delay), `interval(minutes)` (while
  connected; timer resets after each run), `manual` only.
- The scheduler serializes runs across pairs (§2.4). A manual "Sync now"
  jumps the queue.
- Runs never start while a user-initiated transfer queue is active; the
  scheduler waits for the queue to drain.

## 9. Logging and reporting (FR-SYN-7)

Each run appends a record to the pair's history (retained: last 100 runs):
start/end time, trigger, plan summary (counts), per-action results, conflict
list, errors. The Sync view renders the history; the full detail also goes
to the structured app log.

## 10. Test plan hooks

The decision table in §5.1, conflict policy in §5.2, first-run matrix in
§5.3 and folder rules in §5.4 are each covered by scenario tests against the
in-memory fake device (NFR-QLT-3). Mandatory scenarios include: interrupted
run resume (kill between apply actions), checkpoint corruption, unmounted
local root, NFD/NFC roundtrip on a simulated macOS walk, the 1300-entry
listing fallback, and mass-deletion gating.
