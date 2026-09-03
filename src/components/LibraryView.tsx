import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import Dialog from "./Dialog";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import { DownloadIcon, FileIcon, FolderIcon, UploadIcon } from "./icons";
import { useApp } from "../lib/store";
import {
  useBrowse,
  childrenOf,
  subtreeOf,
  folderIdForPath,
  type SortKey,
} from "../lib/browse";
import { ipc, errorMessage, type Entry } from "../lib/ipc";
import { formatBytes, formatDate, formatMonth } from "../lib/format";
import { useT } from "../lib/i18n";
import { useVirtualRows } from "../lib/virtual";

/** Rows grouped under an optional heading (months in the notes view). */
interface RowGroup {
  label: string | null;
  rows: Entry[];
}

/**
 * The library/notes browser (docs/05 §3.2–3.3; FR-BRW-1/2/3/4/5/6):
 * folder navigation, list/grid modes, search across the subtree, sortable
 * columns, multi-select, context actions and OS drag-&-drop upload.
 *
 * With `notes` set, the view flattens the note subtree, sorts by last
 * modified and groups by month, with a "download all new notes" action
 * (docs/05 §3.3; FR-BRW-4).
 */
export default function LibraryView({ notes = false }: { notes?: boolean }) {
  const t = useT();
  const entries = useApp((s) => s.entries);
  const entriesLoading = useApp((s) => s.entriesLoading);
  const toast = useApp((s) => s.toast);
  const browse = useBrowse();

  const [menu, setMenu] = useState<{ x: number; y: number; entry: Entry } | null>(null);
  const [dragOver, setDragOver] = useState(false);
  const lastClicked = useRef<string | null>(null);

  // ---- OS drag & drop (FR-TRF-3) -------------------------------------------
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type === "over" || event.payload.type === "enter") {
        setDragOver(true);
      } else if (event.payload.type === "leave") {
        setDragOver(false);
      } else if (event.payload.type === "drop") {
        setDragOver(false);
        void browse.uploadPaths(event.payload.paths);
      }
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // ---- derive visible rows ---------------------------------------------------
  const search = browse.search.trim().toLowerCase();

  const visible = useMemo(() => {
    if (!entries) return [];
    if (notes) {
      // Notes: the whole subtree flattened to documents, newest first.
      return subtreeOf(entries, browse.root)
        .filter((e) => e.entry_type === "document")
        .filter((e) => !search || e.entry_name.toLowerCase().includes(search))
        .sort((a, b) => (b.modified_date ?? "").localeCompare(a.modified_date ?? ""));
    }
    const pool = search
      ? subtreeOf(entries, browse.root).filter((e) =>
          e.entry_name.toLowerCase().includes(search),
        )
      : childrenOf(entries, browse.path);
    const dir = browse.sortAsc ? 1 : -1;
    return [...pool].sort((a, b) => {
      if (a.entry_type !== b.entry_type) return a.entry_type === "folder" ? -1 : 1;
      switch (browse.sortKey) {
        case "size":
          return ((a.file_size ?? 0) - (b.file_size ?? 0)) * dir;
        case "modified":
          return (a.modified_date ?? "").localeCompare(b.modified_date ?? "") * dir;
        default:
          return (
            a.entry_name.localeCompare(b.entry_name, undefined, {
              numeric: true,
              sensitivity: "base",
            }) * dir
          );
      }
    });
  }, [entries, notes, browse.root, browse.path, search, browse.sortKey, browse.sortAsc]);

  // Month grouping for the notes view (docs/05 §3.3).
  const groups = useMemo<RowGroup[]>(() => {
    if (!notes) return [{ label: null, rows: visible }];
    const out: RowGroup[] = [];
    for (const entry of visible) {
      const label = formatMonth(entry.modified_date);
      const last = out[out.length - 1];
      if (last && last.label === label) last.rows.push(entry);
      else out.push({ label, rows: [entry] });
    }
    return out;
  }, [notes, visible]);

  const newNotes = useMemo(
    () => (notes ? visible.filter((e) => e.is_new).map((e) => e.entry_id) : []),
    [notes, visible],
  );

  // ---- selection ---------------------------------------------------------------
  const onRowClick = (e: React.MouseEvent, entry: Entry) => {
    const id = entry.entry_id;
    if (e.ctrlKey || e.metaKey) {
      browse.setSelection(
        browse.selection.includes(id)
          ? browse.selection.filter((s) => s !== id)
          : [...browse.selection, id],
      );
    } else if (e.shiftKey && lastClicked.current) {
      const ids = visible.map((v) => v.entry_id);
      const a = ids.indexOf(lastClicked.current);
      const b = ids.indexOf(id);
      if (a >= 0 && b >= 0) {
        browse.setSelection(ids.slice(Math.min(a, b), Math.max(a, b) + 1));
      }
    } else {
      browse.setSelection([id]);
    }
    lastClicked.current = id;
  };

  const openEntry = useCallback(
    (entry: Entry) => {
      if (entry.entry_type === "folder") {
        browse.navigate(entry.entry_path);
      } else {
        toast(t("library.opening", { name: entry.entry_name }));
        ipc.openEntry(entry.entry_id).catch((err) => toast(errorMessage(err), "error"));
      }
    },
    [browse, toast, t],
  );

  const onContextMenu = (e: React.MouseEvent, entry: Entry) => {
    e.preventDefault();
    if (!browse.selection.includes(entry.entry_id)) {
      browse.setSelection([entry.entry_id]);
    }
    setMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const menuItems = (entry: Entry): MenuItem[] => {
    const ids = browse.selection.includes(entry.entry_id)
      ? browse.selection
      : [entry.entry_id];
    const single = ids.length === 1;
    return [
      {
        label:
          entry.entry_type === "folder" ? t("library.open") : t("library.openInViewer"),
        disabled: !single,
        onClick: () => openEntry(entry),
      },
      {
        label: t("library.openOnDevice"),
        disabled: !single || entry.entry_type === "folder",
        onClick: () =>
          void ipc
            .openOnDevice(entry.entry_id)
            .catch((err) => toast(errorMessage(err), "error")),
      },
      { label: "", separator: true },
      {
        label:
          ids.length > 1
            ? t("library.downloadN", { n: ids.length })
            : t("library.download"),
        onClick: () => void browse.downloadEntries(ids),
      },
      {
        label: t("library.renameAction"),
        disabled: !single || entry.entry_type === "folder",
        onClick: () => browse.setRenameTarget(entry),
      },
      { label: "", separator: true },
      {
        label:
          ids.length > 1
            ? t("library.deleteN", { n: ids.length })
            : t("library.deleteAction"),
        onClick: () => browse.setDeleteIds(ids),
      },
    ];
  };

  // ---- rendering -----------------------------------------------------------------
  if (!entries) {
    return (
      <div className="flex flex-1 items-center justify-center text-text-secondary">
        {entriesLoading ? t("library.loading") : t("library.notLoaded")}
      </div>
    );
  }

  return (
    <div
      className="relative flex min-h-0 min-w-0 flex-1 flex-col"
      onClick={(e) => {
        if (e.target === e.currentTarget) browse.setSelection([]);
      }}
    >
      {notes && (
        <div className="flex items-center gap-2 border-b border-border px-3 py-1.5">
          <button
            disabled={newNotes.length === 0}
            onClick={() => void browse.downloadEntries(newNotes)}
            className="flex items-center gap-1.5 border border-border px-2.5 py-1 text-[13px] hover:border-text disabled:opacity-40 disabled:hover:border-border"
          >
            <DownloadIcon width={14} height={14} />
            {newNotes.length === 0
              ? t("notes.noNewNotes")
              : newNotes.length === 1
                ? t("notes.downloadOneNew")
                : t("notes.downloadAllNew", { n: newNotes.length })}
          </button>
        </div>
      )}

      {browse.viewMode === "list" ? (
        <ListMode
          groups={groups}
          sortable={!notes}
          showPath={notes || search !== ""}
          selection={browse.selection}
          sortKey={browse.sortKey}
          sortAsc={browse.sortAsc}
          onSort={(k) => browse.setSort(k)}
          onRowClick={onRowClick}
          onOpen={openEntry}
          onContextMenu={onContextMenu}
        />
      ) : (
        <GridMode
          rows={visible}
          selection={browse.selection}
          onRowClick={onRowClick}
          onOpen={openEntry}
          onContextMenu={onContextMenu}
        />
      )}

      {visible.length === 0 && (
        <div className="flex flex-1 items-center justify-center p-8 text-center text-text-secondary">
          {search ? t("library.noSearchResults") : t("library.emptyFolder")}
        </div>
      )}

      {dragOver && (
        <div className="pointer-events-none absolute inset-0 z-30 flex items-center justify-center border-2 border-text bg-bg/80">
          <div className="flex items-center gap-2 text-[15px] font-medium">
            <UploadIcon width={20} height={20} />
            {t("library.dropToUpload", {
              folder: browse.path.split("/").pop() ?? browse.path,
            })}
          </div>
        </div>
      )}

      {browse.busy && (
        <div className="absolute bottom-2 start-2 border border-border bg-surface px-2 py-1 text-xs text-text-secondary">
          {browse.busy}
        </div>
      )}

      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems(menu.entry)}
          onClose={() => setMenu(null)}
        />
      )}

      {browse.deleteIds && (
        <DeleteDialog
          ids={browse.deleteIds}
          entries={entries}
          onClose={() => browse.setDeleteIds(null)}
        />
      )}
      {browse.renameTarget && <RenameDialog entry={browse.renameTarget} />}
      {browse.newFolderOpen && <NewFolderDialog />}
      {browse.conflictPrompt && <ConflictDialog />}
    </div>
  );
}

// ---- list mode -------------------------------------------------------------

/** Fixed row heights for virtualization (NFR-PRF-2). */
const HEADER_ROW_H = 25;
const ENTRY_ROW_H = 36;

/** A group header or an entry, flattened for the virtualized list. */
type FlatRow = { header: string } | { entry: Entry };

function ListMode({
  groups,
  sortable,
  showPath,
  selection,
  sortKey,
  sortAsc,
  onSort,
  onRowClick,
  onOpen,
  onContextMenu,
}: {
  groups: RowGroup[];
  sortable: boolean;
  showPath: boolean;
  selection: string[];
  sortKey: SortKey;
  sortAsc: boolean;
  onSort: (key: SortKey) => void;
  onRowClick: (e: React.MouseEvent, entry: Entry) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (e: React.MouseEvent, entry: Entry) => void;
}) {
  const t = useT();

  // Flatten groups to one row model so a single virtual window covers
  // month headers and entries alike.
  const flat = useMemo<FlatRow[]>(() => {
    const out: FlatRow[] = [];
    for (const group of groups) {
      if (group.label) out.push({ header: group.label });
      for (const entry of group.rows) out.push({ entry });
    }
    return out;
  }, [groups]);
  const heights = useMemo(
    () => flat.map((row) => ("header" in row ? HEADER_ROW_H : ENTRY_ROW_H)),
    [flat],
  );
  const virtual = useVirtualRows(heights);

  const header = (key: SortKey, label: string, extra = "") =>
    sortable ? (
      <button
        onClick={() => onSort(key)}
        className={`flex items-center gap-1 px-2 py-1 text-start text-xs font-semibold uppercase tracking-wide text-text-secondary hover:text-text ${extra}`}
      >
        {label}
        {sortKey === key && <span aria-hidden>{sortAsc ? "▲" : "▼"}</span>}
      </button>
    ) : (
      <span
        className={`px-2 py-1 text-xs font-semibold uppercase tracking-wide text-text-secondary ${extra}`}
      >
        {label}
      </span>
    );

  return (
    <div ref={virtual.containerRef} className="min-h-0 flex-1 overflow-y-auto">
      <div className="sticky top-0 z-10 grid grid-cols-[1fr_7rem_6rem_5rem] border-b border-border bg-surface">
        {header("name", t("library.colName"))}
        {header("modified", t("library.colModified"))}
        {header("size", t("library.colSize"), "justify-end")}
        <span className="px-2 py-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
          {t("library.colPages")}
        </span>
      </div>
      <div
        role="listbox"
        aria-multiselectable
        className="relative"
        style={{ height: virtual.total }}
      >
        {flat.slice(virtual.start, virtual.end).map((row, i) => {
          const idx = virtual.start + i;
          const style: React.CSSProperties = {
            position: "absolute",
            top: virtual.offsets[idx],
            left: 0,
            right: 0,
            height: heights[idx],
          };
          if ("header" in row) {
            return (
              <h3
                key={`h:${row.header}`}
                style={style}
                className="flex items-center border-b border-border bg-surface px-2 text-xs font-semibold text-text-secondary"
              >
                {row.header}
              </h3>
            );
          }
          const entry = row.entry;
          const selected = selection.includes(entry.entry_id);
          return (
            <div
              key={entry.entry_id}
              role="option"
              aria-selected={selected}
              style={style}
              onClick={(e) => onRowClick(e, entry)}
              onDoubleClick={() => onOpen(entry)}
              onContextMenu={(e) => onContextMenu(e, entry)}
              className={`grid cursor-default grid-cols-[1fr_7rem_6rem_5rem] items-center border-b border-border text-[13px] ${
                selected ? "bg-accent text-accent-foreground" : "hover:bg-surface"
              }`}
            >
              <span className="flex min-w-0 items-center gap-2 px-2">
                {entry.entry_type === "folder" ? (
                  <FolderIcon className="shrink-0" />
                ) : (
                  <FileIcon className="shrink-0" />
                )}
                <span className="min-w-0">
                  <span className="block truncate" title={entry.entry_name}>
                    {entry.entry_name}
                    {entry.is_new ? " •" : ""}
                  </span>
                  {showPath && (
                    <span
                      className={`block truncate text-xs ${selected ? "" : "text-text-secondary"}`}
                    >
                      {entry.entry_path}
                    </span>
                  )}
                </span>
              </span>
              <span className="px-2 tabular-nums">
                {formatDate(entry.modified_date)}
              </span>
              <span className="px-2 text-end tabular-nums">
                {entry.entry_type === "folder" ? "—" : formatBytes(entry.file_size)}
              </span>
              <span className="px-2 tabular-nums">
                {entry.total_page != null ? entry.total_page : "—"}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---- grid mode --------------------------------------------------------------

/** Tile metrics for grid virtualization; mirror `minmax(7.5rem,1fr)` + `gap-2`. */
const TILE_MIN_W = 120;
const TILE_H = 88;
const TILE_GAP = 8;
const GRID_PAD = 12; // p-3

function GridMode({
  rows,
  selection,
  onRowClick,
  onOpen,
  onContextMenu,
}: {
  rows: Entry[];
  selection: string[];
  onRowClick: (e: React.MouseEvent, entry: Entry) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (e: React.MouseEvent, entry: Entry) => void;
}) {
  // Row-based virtualization (NFR-PRF-2): compute the column count the way
  // `auto-fill` would, then window over whole tile rows. The width flows
  // through state because the row model must exist before the hook runs.
  const [width, setWidth] = useState(0);
  const cols = Math.max(
    1,
    Math.floor((width - 2 * GRID_PAD + TILE_GAP) / (TILE_MIN_W + TILE_GAP)),
  );
  const rowCount = Math.ceil(rows.length / cols);
  const heights = useMemo(
    () => Array.from({ length: rowCount }, () => TILE_H + TILE_GAP),
    [rowCount],
  );
  const virtual = useVirtualRows(heights);
  useEffect(() => setWidth(virtual.width), [virtual.width]);

  return (
    <div ref={virtual.containerRef} className="min-h-0 flex-1 overflow-y-auto">
      <div className="relative" style={{ height: virtual.total + 2 * GRID_PAD }}>
        {heights.slice(virtual.start, virtual.end).map((_, i) => {
          const rowIdx = virtual.start + i;
          return (
            <div
              key={rowIdx}
              style={{
                position: "absolute",
                top: GRID_PAD + virtual.offsets[rowIdx],
                left: GRID_PAD,
                right: GRID_PAD,
                height: TILE_H,
                display: "grid",
                gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
                gap: TILE_GAP,
              }}
            >
              {rows.slice(rowIdx * cols, (rowIdx + 1) * cols).map((entry) => {
                const selected = selection.includes(entry.entry_id);
                return (
                  <button
                    key={entry.entry_id}
                    onClick={(e) => onRowClick(e, entry)}
                    onDoubleClick={() => onOpen(entry)}
                    onContextMenu={(e) => onContextMenu(e, entry)}
                    aria-pressed={selected}
                    className={`flex h-full flex-col items-center justify-center gap-2 border px-2 ${
                      selected
                        ? "border-text bg-accent text-accent-foreground"
                        : "border-border hover:border-text"
                    }`}
                  >
                    {entry.entry_type === "folder" ? (
                      <FolderIcon width={28} height={28} />
                    ) : (
                      <FileIcon width={28} height={28} />
                    )}
                    <span
                      className="w-full truncate text-center text-xs"
                      title={entry.entry_name}
                    >
                      {entry.entry_name}
                    </span>
                  </button>
                );
              })}
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---- small dialogs -------------------------------------------------------------

function DeleteDialog({
  ids,
  entries,
  onClose,
}: {
  ids: string[];
  entries: Entry[];
  onClose: () => void;
}) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState(false);
  const names = ids
    .map((id) => entries.find((e) => e.entry_id === id)?.entry_name ?? id)
    .slice(0, 5);

  const doDelete = async () => {
    setBusy(true);
    try {
      await ipc.deleteEntries(ids);
      useBrowse.getState().setSelection([]);
      toast(t("deleteDialog.deleted", { n: ids.length }));
      onClose();
    } catch (err) {
      toast(errorMessage(err), "error");
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={
        ids.length > 1
          ? t("deleteDialog.titleMany", { n: ids.length })
          : t("deleteDialog.titleOne")
      }
      onClose={onClose}
    >
      <p className="mb-2">{t("deleteDialog.body")}</p>
      <ul className="mb-4 max-h-32 overflow-y-auto border border-border px-2 py-1 text-xs text-text-secondary">
        {names.map((n) => (
          <li key={n} className="truncate">
            {n}
          </li>
        ))}
        {ids.length > names.length && (
          <li>{t("deleteDialog.more", { n: ids.length - names.length })}</li>
        )}
      </ul>
      <div className="flex justify-end gap-2">
        <button
          onClick={onClose}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("common.cancel")}
        </button>
        <button
          onClick={doDelete}
          disabled={busy}
          className="border-2 border-text bg-bg px-3 py-1.5 font-semibold disabled:opacity-50"
        >
          {busy ? t("common.deleting") : t("common.delete")}
        </button>
      </div>
    </Dialog>
  );
}

function RenameDialog({ entry }: { entry: Entry }) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const close = () => useBrowse.getState().setRenameTarget(null);
  const [name, setName] = useState(entry.entry_name);
  const [busy, setBusy] = useState(false);

  const doRename = async () => {
    setBusy(true);
    try {
      await ipc.renameEntry(entry.entry_id, name.trim());
      close();
    } catch (err) {
      toast(errorMessage(err), "error");
      setBusy(false);
    }
  };

  return (
    <Dialog title={t("renameDialog.title")} onClose={close}>
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && name.trim() && doRename()}
        aria-label={t("renameDialog.title")}
        className="mb-4 w-full border border-border bg-bg px-2 py-1.5 focus:border-text focus:outline-none"
      />
      <div className="flex justify-end gap-2">
        <button
          onClick={close}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("common.cancel")}
        </button>
        <button
          onClick={doRename}
          disabled={busy || name.trim() === "" || name.trim() === entry.entry_name}
          className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
        >
          {t("common.rename")}
        </button>
      </div>
    </Dialog>
  );
}

function NewFolderDialog() {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const path = useBrowse((s) => s.path);
  const close = () => useBrowse.getState().setNewFolderOpen(false);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);

  const doCreate = async () => {
    setBusy(true);
    try {
      const parentId = await folderIdForPath(path);
      await ipc.createRemoteFolder(parentId, name.trim());
      close();
    } catch (err) {
      toast(errorMessage(err), "error");
      setBusy(false);
    }
  };

  return (
    <Dialog title={t("newFolderDialog.title")} onClose={close}>
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && name.trim() && doCreate()}
        placeholder={t("newFolderDialog.placeholder")}
        aria-label={t("newFolderDialog.placeholder")}
        className="mb-4 w-full border border-border bg-bg px-2 py-1.5 placeholder:text-text-secondary focus:border-text focus:outline-none"
      />
      <div className="flex justify-end gap-2">
        <button
          onClick={close}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("common.cancel")}
        </button>
        <button
          onClick={doCreate}
          disabled={busy || name.trim() === ""}
          className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
        >
          {t("common.create")}
        </button>
      </div>
    </Dialog>
  );
}

/** Overwrite / keep-both / skip decision for conflicting uploads (FR-TRF-9). */
function ConflictDialog() {
  const t = useT();
  const prompt = useBrowse((s) => s.conflictPrompt)!;
  const resolve = useBrowse((s) => s.resolveConflicts);
  const close = () => useBrowse.getState().setConflictPrompt(null);

  return (
    <Dialog title={t("conflictDialog.title")} onClose={close}>
      <p className="mb-2">
        {prompt.conflicts.length === 1
          ? t("conflictDialog.bodyOne")
          : t("conflictDialog.bodyMany", { n: prompt.conflicts.length })}
      </p>
      <ul className="mb-4 max-h-32 overflow-y-auto border border-border px-2 py-1 text-xs text-text-secondary">
        {prompt.conflicts.map((c) => (
          <li key={c.existingId} className="truncate">
            {c.fileName}
          </li>
        ))}
      </ul>
      <div className="flex flex-wrap justify-end gap-2">
        <button
          onClick={() => void resolve("skip")}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("conflictDialog.skip")}
        </button>
        <button
          onClick={() => void resolve("keepboth")}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {t("conflictDialog.keepBoth")}
        </button>
        <button
          onClick={() => void resolve("overwrite")}
          className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground"
        >
          {t("conflictDialog.overwrite")}
        </button>
      </div>
    </Dialog>
  );
}
