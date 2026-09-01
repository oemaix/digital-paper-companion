/**
 * Library browsing state and actions (docs/05 §4.2–4.4; FR-BRW-*, FR-TRF-*).
 *
 * One store shared by the toolbar (breadcrumb, search, view toggle, action
 * icons) and the library view. Upload/download flows live here as actions.
 */
import { create } from "zustand";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { ipc, errorMessage, type Entry, type UploadItem } from "./ipc";
import { useApp } from "./store";
import { t } from "./i18n";

export type SortKey = "name" | "modified" | "size";
export type ViewMode = "list" | "grid";

/** A pending upload conflict awaiting a user decision (FR-TRF-9). */
export interface ConflictPrompt {
  destFolderId: string;
  /** Items with no conflict, ready to enqueue as-is. */
  fresh: UploadItem[];
  /** Conflicting items: same name already exists in the folder. */
  conflicts: { localPath: string; fileName: string; existingId: string }[];
}

interface BrowseStore {
  root: string;
  path: string;
  viewMode: ViewMode;
  search: string;
  sortKey: SortKey;
  sortAsc: boolean;
  selection: string[];
  deleteIds: string[] | null;
  conflictPrompt: ConflictPrompt | null;
  renameTarget: Entry | null;
  newFolderOpen: boolean;
  busy: string | null;

  setRoot: (root: string) => void;
  navigate: (path: string) => void;
  setViewMode: (mode: ViewMode) => void;
  setSearch: (search: string) => void;
  setSort: (key: SortKey) => void;
  setSelection: (ids: string[]) => void;
  setDeleteIds: (ids: string[] | null) => void;
  setRenameTarget: (entry: Entry | null) => void;
  setNewFolderOpen: (open: boolean) => void;
  setConflictPrompt: (prompt: ConflictPrompt | null) => void;

  uploadPaths: (paths: string[]) => Promise<void>;
  resolveConflicts: (policy: "overwrite" | "keepboth" | "skip") => Promise<void>;
  pickAndUploadFiles: () => Promise<void>;
  pickAndUploadFolder: () => Promise<void>;
  downloadEntries: (ids: string[]) => Promise<void>;
}

/** Direct children of a device folder path. */
export function childrenOf(entries: Entry[], path: string): Entry[] {
  const prefix = `${path}/`;
  return entries.filter(
    (e) =>
      e.entry_path.startsWith(prefix) && !e.entry_path.slice(prefix.length).includes("/"),
  );
}

/** All entries in the subtree of a device folder path. */
export function subtreeOf(entries: Entry[], path: string): Entry[] {
  const prefix = `${path}/`;
  return entries.filter((e) => e.entry_path.startsWith(prefix));
}

/** Produces `name (2).pdf`, `name (3).pdf`, … avoiding `taken`. */
export function uniqueName(name: string, taken: Set<string>): string {
  if (!taken.has(name)) return name;
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const ext = dot > 0 ? name.slice(dot) : "";
  for (let n = 2; n < 1000; n++) {
    const candidate = `${stem} (${n})${ext}`;
    if (!taken.has(candidate)) return candidate;
  }
  return name;
}

/** Resolves the folder id for a device path, via cache or the device. */
export async function folderIdForPath(path: string): Promise<string> {
  const entries = useApp.getState().entries ?? [];
  const hit = entries.find((e) => e.entry_path === path && e.entry_type === "folder");
  if (hit) return hit.entry_id;
  const resolved = await ipc.resolveFolder(path);
  return resolved.entry_id;
}

export const useBrowse = create<BrowseStore>((set, get) => ({
  root: "Document",
  path: "Document",
  viewMode: "list",
  search: "",
  sortKey: "name",
  sortAsc: true,
  selection: [],
  deleteIds: null,
  conflictPrompt: null,
  renameTarget: null,
  newFolderOpen: false,
  busy: null,

  setRoot: (root) => {
    if (get().root !== root) {
      set({ root, path: root, search: "", selection: [] });
    }
  },
  navigate: (path) => set({ path, search: "", selection: [] }),
  setViewMode: (viewMode) => set({ viewMode }),
  setSearch: (search) => set({ search, selection: [] }),
  setSort: (key) =>
    set((s) =>
      s.sortKey === key ? { sortAsc: !s.sortAsc } : { sortKey: key, sortAsc: true },
    ),
  setSelection: (selection) => set({ selection }),
  setDeleteIds: (deleteIds) => set({ deleteIds }),
  setRenameTarget: (renameTarget) => set({ renameTarget }),
  setNewFolderOpen: (newFolderOpen) => set({ newFolderOpen }),
  setConflictPrompt: (conflictPrompt) => set({ conflictPrompt }),

  /** Routes dropped/picked paths: folders → recursive upload, PDFs → file
   *  upload with conflict detection, anything else → skipped. */
  uploadPaths: async (paths) => {
    const app = useApp.getState();
    if (app.connection.state !== "connected") {
      app.toast(t("browse.connectFirst"), "error");
      return;
    }
    const { path } = get();
    set({ busy: t("browse.preparingUpload") });
    try {
      const classified = await ipc.classifyPaths(paths);
      const destFolderId = await folderIdForPath(path);

      const dirs = classified.filter((c) => c.is_dir);
      const pdfs = classified.filter(
        (c) => !c.is_dir && c.file_name.toLowerCase().endsWith(".pdf"),
      );
      const skipped = classified.length - dirs.length - pdfs.length;
      if (skipped > 0) {
        app.toast(t("browse.skippedNonPdf", { n: skipped }));
      }

      for (const dir of dirs) {
        await ipc.uploadFolder(destFolderId, path, dir.path);
      }

      if (pdfs.length > 0) {
        const entries = app.entries ?? [];
        const siblings = childrenOf(entries, path);
        const byName = new Map(
          siblings
            .filter((e) => e.entry_type === "document")
            .map((e) => [e.entry_name, e]),
        );
        const fresh: UploadItem[] = [];
        const conflicts: ConflictPrompt["conflicts"] = [];
        for (const f of pdfs) {
          const existing = byName.get(f.file_name);
          if (existing) {
            conflicts.push({
              localPath: f.path,
              fileName: f.file_name,
              existingId: existing.entry_id,
            });
          } else {
            fresh.push({ local_path: f.path, file_name: f.file_name });
          }
        }
        if (conflicts.length > 0) {
          set({ conflictPrompt: { destFolderId, fresh, conflicts } });
        } else if (fresh.length > 0) {
          await ipc.uploadFiles(destFolderId, fresh);
        }
      }
    } catch (err) {
      app.toast(errorMessage(err), "error");
    } finally {
      set({ busy: null });
    }
  },

  /** Applies the user's conflict decision and enqueues the uploads. */
  resolveConflicts: async (policy) => {
    const prompt = get().conflictPrompt;
    if (!prompt) return;
    set({ conflictPrompt: null });
    const app = useApp.getState();
    try {
      const items: UploadItem[] = [...prompt.fresh];
      if (policy === "overwrite") {
        for (const c of prompt.conflicts) {
          items.push({
            local_path: c.localPath,
            file_name: c.fileName,
            existing_doc_id: c.existingId,
          });
        }
      } else if (policy === "keepboth") {
        const entries = app.entries ?? [];
        const taken = new Set(childrenOf(entries, get().path).map((e) => e.entry_name));
        for (const c of prompt.conflicts) {
          const name = uniqueName(c.fileName, taken);
          taken.add(name);
          items.push({ local_path: c.localPath, file_name: name });
        }
      }
      if (items.length > 0) {
        await ipc.uploadFiles(prompt.destFolderId, items);
      }
    } catch (err) {
      app.toast(errorMessage(err), "error");
    }
  },

  pickAndUploadFiles: async () => {
    const picked = await openFileDialog({
      multiple: true,
      filters: [{ name: "PDF documents", extensions: ["pdf"] }],
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (paths.length > 0) await get().uploadPaths(paths);
  },

  pickAndUploadFolder: async () => {
    const picked = await openFileDialog({ directory: true });
    if (!picked || Array.isArray(picked)) return;
    await get().uploadPaths([picked]);
  },

  /** Asks for a target directory and enqueues downloads (FR-TRF-2/5). */
  downloadEntries: async (ids) => {
    if (ids.length === 0) return;
    const dir = await openFileDialog({ directory: true, title: t("browse.downloadTo") });
    if (!dir || Array.isArray(dir)) return;
    try {
      await ipc.downloadEntries(ids, dir);
    } catch (err) {
      useApp.getState().toast(errorMessage(err), "error");
    }
  },
}));
