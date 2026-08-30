import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import Dialog from "./Dialog";
import ContextMenu, { type MenuItem } from "./ContextMenu";
import { FileIcon, FolderIcon, UploadIcon } from "./icons";
import { useApp } from "../lib/store";
import {
  useBrowse,
  childrenOf,
  subtreeOf,
  folderIdForPath,
  type SortKey,
} from "../lib/browse";
import { ipc, errorMessage, type Entry } from "../lib/ipc";
import { formatBytes, formatDate } from "../lib/format";

/**
 * The library/notes browser (docs/05 §4.2–4.3; FR-BRW-1/2/3/5/6):
 * folder navigation, list/grid modes, search across the subtree, sortable
 * columns, multi-select, context actions and OS drag-&-drop upload.
 */
export default function LibraryView() {
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
  const visible = useMemo(() => {
    if (!entries) return [];
    const pool = browse.search.trim()
      ? subtreeOf(entries, browse.root).filter((e) =>
          e.entry_name.toLowerCase().includes(browse.search.trim().toLowerCase()),
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
  }, [entries, browse.root, browse.path, browse.search, browse.sortKey, browse.sortAsc]);

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
        toast(`Opening "${entry.entry_name}"…`);
        ipc.openEntry(entry.entry_id).catch((err) => toast(errorMessage(err), "error"));
      }
    },
    [browse, toast],
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
        label: entry.entry_type === "folder" ? "Open" : "Open in PDF viewer",
        disabled: !single,
        onClick: () => openEntry(entry),
      },
      {
        label: "Open on device",
        disabled: !single || entry.entry_type === "folder",
        onClick: () =>
          void ipc
            .openOnDevice(entry.entry_id)
            .catch((err) => toast(errorMessage(err), "error")),
      },
      { label: "", separator: true },
      {
        label: ids.length > 1 ? `Download ${ids.length} items…` : "Download…",
        onClick: () => void browse.downloadEntries(ids),
      },
      {
        label: "Rename…",
        disabled: !single || entry.entry_type === "folder",
        onClick: () => browse.setRenameTarget(entry),
      },
      { label: "", separator: true },
      {
        label: ids.length > 1 ? `Delete ${ids.length} items…` : "Delete…",
        onClick: () => browse.setDeleteIds(ids),
      },
    ];
  };

  // ---- rendering -----------------------------------------------------------------
  if (!entries) {
    return (
      <div className="flex flex-1 items-center justify-center text-text-secondary">
        {entriesLoading ? "Loading library…" : "Library not loaded."}
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
      {browse.viewMode === "list" ? (
        <ListMode
          rows={visible}
          showPath={browse.search.trim() !== ""}
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
          {browse.search.trim()
            ? "No entries match the search."
            : "This folder is empty. Drag PDF files here to upload them."}
        </div>
      )}

      {dragOver && (
        <div className="pointer-events-none absolute inset-0 z-30 flex items-center justify-center border-2 border-text bg-bg/80">
          <div className="flex items-center gap-2 text-[15px] font-medium">
            <UploadIcon width={20} height={20} />
            Drop to upload into “{browse.path.split("/").pop()}”
          </div>
        </div>
      )}

      {browse.busy && (
        <div className="absolute bottom-2 left-2 border border-border bg-surface px-2 py-1 text-xs text-text-secondary">
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

function ListMode({
  rows,
  showPath,
  selection,
  sortKey,
  sortAsc,
  onSort,
  onRowClick,
  onOpen,
  onContextMenu,
}: {
  rows: Entry[];
  showPath: boolean;
  selection: string[];
  sortKey: SortKey;
  sortAsc: boolean;
  onSort: (key: SortKey) => void;
  onRowClick: (e: React.MouseEvent, entry: Entry) => void;
  onOpen: (entry: Entry) => void;
  onContextMenu: (e: React.MouseEvent, entry: Entry) => void;
}) {
  const header = (key: SortKey, label: string, extra = "") => (
    <button
      onClick={() => onSort(key)}
      className={`flex items-center gap-1 px-2 py-1 text-left text-xs font-semibold uppercase tracking-wide text-text-secondary hover:text-text ${extra}`}
    >
      {label}
      {sortKey === key && <span aria-hidden>{sortAsc ? "▲" : "▼"}</span>}
    </button>
  );

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="sticky top-0 z-10 grid grid-cols-[1fr_7rem_6rem_5rem] border-b border-border bg-surface">
        {header("name", "Name")}
        {header("modified", "Modified")}
        {header("size", "Size", "justify-end")}
        <span className="px-2 py-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
          Pages
        </span>
      </div>
      <ul role="listbox" aria-multiselectable>
        {rows.map((entry) => {
          const selected = selection.includes(entry.entry_id);
          return (
            <li
              key={entry.entry_id}
              role="option"
              aria-selected={selected}
              onClick={(e) => onRowClick(e, entry)}
              onDoubleClick={() => onOpen(entry)}
              onContextMenu={(e) => onContextMenu(e, entry)}
              className={`grid h-9 cursor-default grid-cols-[1fr_7rem_6rem_5rem] items-center border-b border-border text-[13px] ${
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
              <span className="px-2 tabular-nums">{formatDate(entry.modified_date)}</span>
              <span className="px-2 text-right tabular-nums">
                {entry.entry_type === "folder" ? "—" : formatBytes(entry.file_size)}
              </span>
              <span className="px-2 tabular-nums">
                {entry.total_page != null ? entry.total_page : "—"}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

// ---- grid mode --------------------------------------------------------------

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
  return (
    <div className="min-h-0 flex-1 overflow-y-auto p-3">
      <div className="grid grid-cols-[repeat(auto-fill,minmax(7.5rem,1fr))] gap-2">
        {rows.map((entry) => {
          const selected = selection.includes(entry.entry_id);
          return (
            <button
              key={entry.entry_id}
              onClick={(e) => onRowClick(e, entry)}
              onDoubleClick={() => onOpen(entry)}
              onContextMenu={(e) => onContextMenu(e, entry)}
              className={`flex flex-col items-center gap-2 border px-2 py-3 ${
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
      toast(`Deleted ${ids.length} item${ids.length > 1 ? "s" : ""}`);
      onClose();
    } catch (err) {
      toast(errorMessage(err), "error");
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={`Delete ${ids.length > 1 ? `${ids.length} items` : "item"}`}
      onClose={onClose}
    >
      <p className="mb-2">
        This permanently deletes from the device (folders including their contents). This
        cannot be undone.
      </p>
      <ul className="mb-4 max-h-32 overflow-y-auto border border-border px-2 py-1 text-xs text-text-secondary">
        {names.map((n) => (
          <li key={n} className="truncate">
            {n}
          </li>
        ))}
        {ids.length > names.length && <li>… and {ids.length - names.length} more</li>}
      </ul>
      <div className="flex justify-end gap-2">
        <button
          onClick={onClose}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          Cancel
        </button>
        <button
          onClick={doDelete}
          disabled={busy}
          className="border-2 border-text bg-bg px-3 py-1.5 font-semibold disabled:opacity-50"
        >
          {busy ? "Deleting…" : "Delete"}
        </button>
      </div>
    </Dialog>
  );
}

function RenameDialog({ entry }: { entry: Entry }) {
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
    <Dialog title="Rename" onClose={close}>
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && name.trim() && doRename()}
        className="mb-4 w-full border border-border bg-bg px-2 py-1.5 focus:border-text focus:outline-none"
      />
      <div className="flex justify-end gap-2">
        <button
          onClick={close}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          Cancel
        </button>
        <button
          onClick={doRename}
          disabled={busy || name.trim() === "" || name.trim() === entry.entry_name}
          className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
        >
          Rename
        </button>
      </div>
    </Dialog>
  );
}

function NewFolderDialog() {
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
    <Dialog title="New folder" onClose={close}>
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && name.trim() && doCreate()}
        placeholder="Folder name"
        className="mb-4 w-full border border-border bg-bg px-2 py-1.5 placeholder:text-text-secondary focus:border-text focus:outline-none"
      />
      <div className="flex justify-end gap-2">
        <button
          onClick={close}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          Cancel
        </button>
        <button
          onClick={doCreate}
          disabled={busy || name.trim() === ""}
          className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
        >
          Create
        </button>
      </div>
    </Dialog>
  );
}

/** Overwrite / keep-both / skip decision for conflicting uploads (FR-TRF-9). */
function ConflictDialog() {
  const prompt = useBrowse((s) => s.conflictPrompt)!;
  const resolve = useBrowse((s) => s.resolveConflicts);
  const close = () => useBrowse.getState().setConflictPrompt(null);

  return (
    <Dialog title="Files already exist" onClose={close}>
      <p className="mb-2">
        {prompt.conflicts.length === 1
          ? "One file already exists in this folder:"
          : `${prompt.conflicts.length} files already exist in this folder:`}
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
          Skip
        </button>
        <button
          onClick={() => void resolve("keepboth")}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          Keep both
        </button>
        <button
          onClick={() => void resolve("overwrite")}
          className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground"
        >
          Overwrite
        </button>
      </div>
    </Dialog>
  );
}
