# 05 — UI/UX Specification

## 1. Design principles

1. **Content first.** The document library is the home screen; device
   plumbing (connection, settings) stays out of the way once set up.
2. **Always show state.** Connection, transfer and sync status are visible
   at a glance from anywhere in the app.
3. **Direct manipulation.** Drag & drop is the primary transfer gesture;
   menus and buttons are the fallback, never the only way.
4. **Calm surfaces.** Generous whitespace, one accent color, subtle motion.
   E-ink owners chose a distraction-free device; the companion should match.
5. **No dead ends.** Every empty state and every error names the next step.

## 2. Application layout

```
┌────────────────────────────────────────────────────────────┐
│ Titlebar (native or custom per platform)                   │
├──────────┬─────────────────────────────────────────────────┤
│          │  Toolbar: breadcrumb · search · view toggle ·   │
│ Sidebar  │           upload · new folder · sync now        │
│          ├─────────────────────────────────────────────────┤
│ Library  │                                                 │
│  Notes   │            Main content area                    │
│  Templts │      (file browser / notes / templates /        │
│ Sync     │        sync / settings / device)                │
│ Device   │                                                 │
│ Settings │                                                 │
├──────────┴─────────────────────────────────────────────────┤
│ Status bar: device name + connection dot · storage/battery │
│             · active transfer/sync summary (clickable)     │
└────────────────────────────────────────────────────────────┘
```

- **Sidebar** (fixed, icons + labels): Library, Notes, Templates, Sync,
  Device, Settings. A device selector sits at the top when multiple devices
  are registered (FR-CONN-7).
- **Status bar** is permanent. The transfer/sync summary expands into the
  **Activity panel** (slide-over) listing queued/running/finished jobs with
  progress and cancel/retry controls (FR-TRF-7).

## 3. Screens

### 3.1 Welcome & pairing wizard (first run / no device)

Full-window flow, one step per screen, progress dots on top:

1. **Welcome** — one-paragraph pitch, button "Find my device".
2. **Find device** — live list of mDNS discoveries (model, serial, address)
   with a spinner; footer links: "Enter address manually", "Connect via
   USB cable" (runs the USB mode-switch flow with plain-language guidance),
   "Import credentials from the Sony app" (FR-REG-6).
3. **Pairing** — instructs the user to look at the device screen; large PIN
   input (digit boxes) in the app; clear error + retry on failure
   (FR-REG-2).
4. **Done** — device card (name, model, serial, battery, storage), primary
   button "Open library", secondary "Set up sync now".

### 3.2 Library (home)

- **Views:** list (default; columns Name, Size, Pages, Modified, sortable)
  and grid (large file-type tiles). Toggle in toolbar (FR-BRW-2).
- **Breadcrumb** path with dropdown per segment for fast up-navigation.
- **Search field** filters the cached tree live; results show full paths
  (FR-BRW-3).
- **Unread dot** on entries with `is_new = "true"`.
- **Selection model:** click, Ctrl/Cmd-click, Shift-range, Ctrl/Cmd-A.
- **Context menu (document):** Preview · Open on device · Download… ·
  Rename · Move… · Copy… · Delete.
- **Context menu (folder):** Open · Download folder… · Rename · Move… ·
  Delete · New subfolder.
- **Drag & drop:** OS → app uploads into the hovered folder (drop highlight);
  app-internal drag moves entries between folders (modifier key = copy).
- **Empty folder state:** dashed drop-zone illustration, "Drop PDFs here or
  click Upload".
- **Overwrite dialog** on name collisions with Overwrite / Keep both / Skip
  and "Apply to all" (FR-TRF-3).

### 3.3 Notes

Filtered view of the device's note folder: sorted by last modified,
grouped by month. Same actions as Library plus prominent "Download all new
notes". (FR-BRW-4)

### 3.4 Templates

Grid of template cards (name; thumbnail post-v1). Actions: Add template
(file picker or drop a PDF), Rename?—not supported by protocol → omitted—,
Delete. Uploading shows the same queue as documents. (FR-BRW-7, FR-TRF-6)

### 3.5 Sync

- **Pair list:** each sync pair as a card — local path ⇄ device subtree,
  mode icon (two-way / mirror→PC / mirror→device), schedule summary, last
  run result, buttons **Sync now**, **Preview**, Edit, per-pair history.
- **Pair editor (modal):** local folder picker · device subtree picker ·
  mode selector with one-line explanations · schedule (on connect /
  every N minutes / manual only) · filters (exclude globs) · mass-deletion
  threshold.
- **Preview screen:** three columns (Uploads, Downloads, Deletions) with
  per-item checkboxes, then "Apply". (FR-SYN-5)
- **Run view:** live progress (current file, n of m, throughput), cancel
  button; on completion an inline summary linking to the log (FR-SYN-7).
- **Conflict badge** on files resolved as conflicts, with an explainer of
  where the conflict copy lives.

### 3.6 Device

- **Status cards:** battery (level + charging), storage (bar, used/total),
  model/serial/firmware/MAC, connection transport.
- **Actions:** Take screenshot (shows result with Save/Copy), Open document
  on device (picker + page), Set clock from computer.
- **Wi-Fi section:** radio toggle, stored networks (with remove), scan +
  join dialog (password field, advanced static-IP disclosure). (FR-SET-4)

### 3.7 Settings (app + device)

Two tabs:

- **Application:** theme (system/light/dark), language, start behavior
  (launch at login, start minimized to tray), tray on/off, update check
  toggle, log level + "Reveal log file", "Forget this device".
- **Device configuration:** owner name, timezone, date/time format, standby
  timeout — plus collapsible **Advanced** table of all remaining
  `/system/configs/` keys as raw key/value editors (FR-SET-2).

### 3.8 System tray / menu bar (FR-APP-2)

Icon reflects state (connected / disconnected / syncing). Menu: device name
+ status, Sync now (per pair), Open Digital Paper Companion, Quit.

## 4. Key flows

### 4.1 Daily use

Open app → auto-connect (status dot turns green, library loads from cache
then refreshes) → user drags three PDFs onto "Papers" → queue chip appears
in status bar, fills, turns into "3 uploaded ✓" → scheduled on-connect sync
had already pulled yesterday's annotated notes into `~/DigitalPaper/Note`.

### 4.2 Connection loss mid-transfer

Transfer fails → job marked *Failed (device unreachable)* with Retry →
status dot yellow *reconnecting…* → on reconnect a toast offers "Retry 2
failed transfers". No partial file exists at the destination (NFR-REL-2).

### 4.3 Device was factory-reset

Connect fails pinning/auth → dialog: "This device doesn't recognize this
computer anymore (it may have been reset). Pair again?" → wizard step 3
directly, replacing stored credentials on success (NFR-SEC-2).

## 5. Design system

### 5.1 Foundations

| Token | Light | Dark |
|---|---|---|
| Background | `#FAFAF8` (warm paper white) | `#111315` |
| Surface / card | `#FFFFFF` | `#1A1D1F` |
| Border | `#E4E4E0` | `#2A2E31` |
| Text primary | `#1C1E21` | `#ECEDEE` |
| Text secondary | `#6B6F76` | `#9BA1A6` |
| Accent | `#2563EB` (blue 600) | `#3B82F6` |
| Success / Warning / Danger | `#16A34A` / `#D97706` / `#DC2626` | brightened variants |

- The light palette deliberately leans paper-warm to echo the device.
- **Typography:** Inter (bundled). Sizes 13 px base UI, 15 px list rows,
  20/24 px headings. Tabular numerals for sizes and dates.
- **Spacing:** 4 px grid; radii 8 px (cards, inputs), 6 px (buttons).
- **Icons:** Lucide set, 16/20 px, 1.5 px stroke.
- **Motion:** 120–180 ms ease-out for panels/toasts; progress bars animate
  smoothly; no decorative animation.
- **Density:** comfortable default; list rows 36 px.

### 5.2 Components

Buttons (primary/secondary/ghost/danger) · toolbar · breadcrumb · virtualized
table & grid · tree picker · modal · slide-over (Activity) · toast (bottom
right, auto-dismiss except errors) · progress (bar + circular) · badge/dot ·
empty state (illustration + one action) · segmented control · switch · form
fields with inline validation.

### 5.3 Platform conventions

- Shortcuts: `Cmd`/`Ctrl` + `U` upload, `D` download, `S` sync now, `F`
  search, `R` refresh, `Del`/`Cmd-Backspace` delete, `Enter` open/preview,
  `F2`/`Enter`(mac) rename.
- macOS: traffic-light inset custom titlebar; Windows/Linux: native or
  Tauri decorations. Menus follow platform placement (app menu on macOS).
- File dialogs, notifications and tray are always the native ones.

## 6. Content and tone

- Plain, short sentences. Verbs on buttons ("Upload", "Sync now", not "OK").
- Errors: *what happened* + *what to do*. Example: "Couldn't reach DPT-RP1
  (no response at 10.0.1.12). Check that the device's Wi-Fi is on, then
  Retry."
- Never blame the user; never show raw protocol errors outside the log.
- Destructive confirmations name the object: "Delete 'thesis-v3.pdf' from
  the device? This can't be undone." (NFR-UX-5)
