# 05 — UI/UX Specification

## 1. Design principles

1. **Content first.** The document library is the home screen; device
   plumbing (connection, settings) stays out of the way once set up.
2. **Views vs. actions.** The sidebar holds only *content views* (Library,
   Notes, Templates). Everything the user *does to* the app or device —
   connect, sync now, settings — is an **action**: an icon in the toolbar
   that triggers directly or opens a dialog. Actions are never sidebar
   destinations.
3. **Always show state.** Connection, transfer and sync status are visible
   at a glance from anywhere in the app (status bar).
4. **Direct manipulation.** Drag & drop is the primary transfer gesture;
   menus and buttons are the fallback, never the only way.
5. **Monochrome, like the device.** The entire UI is pure black and white
   (plus grays); no hue anywhere, sharp corners everywhere. State is
   communicated through shape, weight, icons and text — never color. This
   mirrors the e-ink device and satisfies NFR-UX-4 by construction.
6. **No dead ends.** Every empty state and every error names the next step.

## 2. Application layout

```
┌────────────────────────────────────────────────────────────┐
│ Titlebar (native or custom per platform)                   │
├────────────────────────────────────────────────────────────┤
│ Toolbar:  breadcrumb (path of current folder) · search ·   │
│           view toggle (list/icons) · refresh │ new folder · │
│           upload · download · delete │ ⇄ sync · ⚡connect ·  │
│           ⚙ settings │ ⇅ transfers                          │
├──────────┬─────────────────────────────────────────────────┤
│ Sidebar  │                                                 │
│          │                                                 │
│ Library  │            Main content area                    │
│ Notes    │      (file browser / notes / templates)         │
│ Templates│                                                 │
│          │                                                 │
├──────────┴─────────────────────────────────────────────────┤
│ Status bar: connection dot+text · sync status/progress ·   │
│             storage/battery · transfer summary (clickable) │
└────────────────────────────────────────────────────────────┘
```

- **Sidebar** (fixed, icons + labels) holds the three content views only:
  **Library, Notes, Templates**. A device selector sits at the top when
  multiple devices are registered (FR-CONN-7).
- **Toolbar** (full width, above sidebar + content) has three zones:
  - *Navigation:* breadcrumb of the current folder path (per-segment
    dropdown for fast up-navigation) and the search field;
  - *View controls:* list/icon view toggle, sort menu — these apply to the
    active content view;
  - *Actions* (right-aligned monochrome icons with tooltips):
    four groups, left to right: **look** (view toggle + refresh),
    **this folder** (new folder, upload, download, delete of the current
    selection), **device/app** (Sync, Connect, Settings), **activity**
    (Transfers, far right with a count badge). Download asks for a
    target directory, then enqueues the selected files and folders.
    Delete opens a confirmation dialog, then removes the items on the
    device.
    Sync is a spinning icon while a run is in progress, same pattern
    as Refresh. Context actions appear when a folder view is active.
- **Status bar** is permanent: connection state (filled dot = connected,
  hollow = disconnected, half = reconnecting — never color-coded), current
  sync status/progress, battery/storage, and the transfer summary, which
  expands into the **Activity panel** (slide-over) listing
  queued/running/finished jobs with progress and cancel/retry controls
  (FR-TRF-7).
- **Dialogs** carry all configuration and guided flows (connect/pairing,
  settings, sync preview/confirmations). Sharp-cornered modal panels,
  closable via ×, backdrop click and Escape.

## 3. Screens

### 3.1 Connect & pairing dialog

Opened from the toolbar **Connect** icon, or from the empty state's
"Connect to device" button on first run. A stepped dialog (progress dots on
top); registration is not a separate entry point — it is the continuation
of connecting to a device that doesn't know this client yet:

1. **Find device** — live list of mDNS discoveries (model, serial, address)
   with a spinner; plus a manual address field (IP, hostname, IPv6 with
   zone) for FR-CONN-2; footer links: "Connect via USB cable" (runs the
   USB mode-switch flow with plain-language guidance), "Import credentials
   from the Sony app" (FR-REG-6).
2. **Pairing** (only when the device requires registration) — instructs the
   user to look at the device screen; large PIN input (digit boxes) in the
   dialog; clear error + retry on failure (FR-REG-2).
3. **Done** — device card (name, model, serial, battery, storage), primary
   button "Open library", secondary "Set up sync now" (jumps to the
   settings dialog, Sync tab).

Known, reachable devices connect directly from step 1 without further
steps; the dialog closes and the status bar dot fills.

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

### 3.5 Sync (one-click action + status bar)

Sync is **not a view**. The toolbar **Sync now** icon triggers a run of the
configured sync pairs (FR-SYN-3); everything else lives in the status bar
and dialogs:

- **Status bar** shows the live sync state: `Sync: idle` /
  `Sync: running — 12/87 files` (with a thin progress bar) / last result
  with timestamp. Clicking it opens the Activity panel with per-action
  detail, cancel, and the run history/log (FR-SYN-7).
- **Configuration** (pairs, mode, schedule, filters) lives in the settings
  dialog, Sync tab (§3.6).
- **Preview (dry run)** opens as a dialog: three columns (Uploads,
  Downloads, Deletions) with per-item checkboxes, then "Apply" (FR-SYN-5).
  The mass-deletion confirmation uses the same dialog.
- **Conflict badge** on files resolved as conflicts, with an explainer of
  where the conflict copy lives.

### 3.6 Settings dialog

Opened from the toolbar **Settings** icon. Three tabs:

- **Application:** theme (system/light/dark), language, start behavior
  (launch at login, start minimized to tray), tray on/off, update check
  toggle, log level + "Reveal log file", "Forget this device".
- **Device:** status block (battery, storage bar, model/serial/firmware/
  MAC, connection transport) and actions (Take screenshot with Save/Copy,
  Set clock from computer); configuration: owner name, timezone, date/time
  format, standby timeout; Wi-Fi management (radio toggle, stored networks,
  scan + join with password field and advanced static-IP disclosure,
  FR-SET-4); collapsible **Advanced** table of all remaining
  `/system/configs/` keys as raw key/value editors (FR-SET-2).
- **Sync:** the pair list — each sync pair as a row: local path ⇄ device
  subtree, mode icon (two-way / mirror→PC / mirror→device), schedule
  summary, last run result, buttons **Sync now**, **Preview**, Edit,
  per-pair history. The pair editor (local folder picker · device subtree
  picker · mode selector with one-line explanations · schedule · exclude
  filters · mass-deletion threshold) opens in place.

### 3.7 System tray / menu bar (FR-APP-2)

Icon reflects state (connected / disconnected / syncing). Menu: device name
+ status, Sync now (per pair), Open Digital Paper Companion, Quit.

## 4. Key flows

### 4.1 Daily use

Open app → auto-connect (status dot fills, library loads from cache then
refreshes) → user drags three PDFs onto "Papers" → queue chip appears in
status bar, fills, turns into "3 uploaded ✓" → scheduled on-connect sync
had already pulled yesterday's annotated notes into `~/DigitalPaper/Note`.

### 4.2 Connection loss mid-transfer

Transfer fails → job marked *Failed (device unreachable)* with Retry →
status dot half-filled with text *reconnecting…* → on reconnect a toast
offers "Retry 2 failed transfers". No partial file exists at the
destination (NFR-REL-2).

### 4.3 Device was factory-reset

Connect fails pinning/auth → dialog: "This device doesn't recognize this
computer anymore (it may have been reset). Pair again?" → connect dialog
at the pairing step (§3.1 step 2), replacing stored credentials on success
(NFR-SEC-2).

## 5. Design system

### 5.1 Foundations

The palette is **pure monochrome** — black, white and grays only, echoing
the e-ink device. There are no hue-based status colors; success, warning
and danger are expressed through icons (✓, !, ×), text and weight.

| Token | Light | Dark |
|---|---|---|
| Background | `#FFFFFF` | `#000000` |
| Surface (toolbar, sidebar, status bar) | `#F6F6F5` | `#131313` |
| Border | `#D9D9D6` | `#2E2E2E` |
| Text primary | `#000000` | `#FFFFFF` |
| Text secondary | `#5C5C5C` | `#A3A3A3` |
| Accent (selection, primary buttons) | `#000000` on white | `#FFFFFF` on black |
| Accent foreground | `#FFFFFF` | `#000000` |

- Dark theme is a pure inversion; both follow the OS setting with a manual
  override in settings.
- **Corners are sharp everywhere** (border-radius 0) — cards, buttons,
  inputs, dialogs — matching the device's hardware language. The only
  circles are the status dot and the app icon's home button.
- **Typography:** Inter (bundled). Sizes 13 px base UI, 15 px list rows,
  20/24 px headings. Tabular numerals for sizes and dates.
- **Spacing:** 4 px grid.
- **Icons:** monochrome stroke set (`src/components/icons.tsx`, Lucide-
  derived outlines), 16/20 px, 1.5 px stroke, `currentColor`; rectangles
  inside icons use sharp corners.
- **Motion:** 120–180 ms ease-out for panels/toasts; progress bars animate
  smoothly; no decorative animation.
- **Density:** comfortable default; list rows 36 px.

### 5.1a App icon

Monochrome mark: a black e-reader with **sharp corners** on white — bezel,
white screen with text lines and a handwriting stroke, circular home
button. Source of truth is `assets/app-icon.svg`; platform formats are
generated via `rsvg-convert` + `npm run tauri icon` (see the comment in the
SVG).

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
