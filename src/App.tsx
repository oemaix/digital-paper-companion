import { useEffect, useMemo, useState } from "react";
import ConnectDialog from "./components/ConnectDialog";
import SettingsDialog, { type SettingsTab } from "./components/SettingsDialog";
import SyncConfirmDialog from "./components/SyncConfirmDialog";
import LibraryView from "./components/LibraryView";
import TransfersPanel from "./components/TransfersPanel";
import ContextMenu from "./components/ContextMenu";
import Toasts from "./components/Toasts";
import {
  ChevronRightIcon,
  ConnectIcon,
  DownloadIcon,
  FolderIcon,
  FolderPlusIcon,
  GridIcon,
  ListIcon,
  NoteIcon,
  RefreshIcon,
  SettingsIcon,
  SyncIcon,
  TemplateIcon,
  TransfersIcon,
  TrashIcon,
  UploadIcon,
} from "./components/icons";
import { useApp } from "./lib/store";
import { useBrowse } from "./lib/browse";
import { ipc, errorMessage } from "./lib/ipc";

/**
 * Application shell per docs/05 §2:
 * - sidebar: content views (Library, Notes, Templates)
 * - toolbar: breadcrumb, search, view-mode toggle, action icons
 * - status bar: connection state, transfer summary, version
 */

const VIEWS = [
  { id: "library", label: "Library", Icon: FolderIcon, root: "Document" },
  { id: "notes", label: "Notes", Icon: NoteIcon, root: "Document/Note" },
  { id: "templates", label: "Templates", Icon: TemplateIcon, root: null },
] as const;

type ViewId = (typeof VIEWS)[number]["id"];
type DialogId = "connect" | "settings" | null;

export default function App() {
  const app = useApp();
  const browse = useBrowse();
  const [activeView, setActiveView] = useState<ViewId>("library");
  const [dialog, setDialog] = useState<DialogId>(null);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("application");
  const [transfersOpen, setTransfersOpen] = useState(false);
  const [uploadMenu, setUploadMenu] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    void app.init();
  }, []);

  const connected = app.connection.state === "connected";
  const view = VIEWS.find((v) => v.id === activeView)!;
  const browsable = connected && view.root !== null;

  const switchView = (id: ViewId) => {
    setActiveView(id);
    const root = VIEWS.find((v) => v.id === id)?.root;
    if (root) browse.setRoot(root);
  };

  // Breadcrumb segments relative to the view root.
  const crumbs = useMemo(() => {
    if (!view.root) return [];
    const rel = browse.path.slice(view.root.length).split("/").filter(Boolean);
    const out: { label: string; path: string }[] = [
      { label: view.label, path: view.root },
    ];
    let acc: string = view.root;
    for (const seg of rel) {
      acc = `${acc}/${seg}`;
      out.push({ label: seg, path: acc });
    }
    return out;
  }, [view, browse.path]);

  const activeTransfers = app.transfers.filter(
    (t) => t.status === "queued" || t.status === "running",
  );
  const runningJob = app.transfers.find((t) => t.status === "running");

  const openSettings = (tab: SettingsTab) => {
    setSettingsTab(tab);
    setDialog("settings");
  };

  const syncPairCount = app.syncPairs?.length ?? 0;
  const syncRunning = app.syncStatus.running;
  // Most recent run: live event wins, otherwise the newest persisted record.
  const lastSync = useMemo(() => {
    const persisted = (app.syncPairs ?? [])
      .map((p) => p.last_run)
      .filter((r): r is NonNullable<typeof r> => !!r)
      .sort((a, b) => b.finished_at.localeCompare(a.finished_at))[0];
    if (app.lastSyncRecord && persisted) {
      return app.lastSyncRecord.finished_at >= persisted.finished_at
        ? app.lastSyncRecord
        : persisted;
    }
    return app.lastSyncRecord ?? persisted ?? null;
  }, [app.syncPairs, app.lastSyncRecord]);

  const syncNow = () => {
    if (syncPairCount === 0) {
      openSettings("sync");
      return;
    }
    void ipc.syncRunAll().then(
      (n) =>
        app.toast(
          n === 0
            ? "No enabled sync pairs"
            : `Sync started (${n} pair${n === 1 ? "" : "s"})`,
        ),
      (err) => app.toast(errorMessage(err), "error"),
    );
  };

  const connectionLabel: Record<string, string> = {
    disconnected: "Not connected",
    connecting: "Connecting…",
    connected: app.connection.name ?? "Connected",
    reauthenticating: "Reconnecting…",
  };

  const toolbarButton =
    "flex items-center gap-1.5 border border-transparent p-1.5 text-text-secondary hover:border-border hover:text-text disabled:opacity-40 disabled:hover:border-transparent";

  return (
    <div className="relative flex h-screen flex-col">
      {/* Toolbar */}
      <header className="flex h-10 shrink-0 items-center gap-1 border-b border-border bg-surface px-2">
        {/* Breadcrumb */}
        <nav className="flex min-w-0 flex-1 items-center gap-0.5 text-[13px]">
          {crumbs.length === 0 ? (
            <span className="px-1 font-medium">{view.label}</span>
          ) : (
            crumbs.map((c, i) => (
              <span key={c.path} className="flex min-w-0 items-center gap-0.5">
                {i > 0 && (
                  <ChevronRightIcon
                    className="shrink-0 text-text-secondary"
                    width={12}
                    height={12}
                  />
                )}
                <button
                  onClick={() => browse.navigate(c.path)}
                  className={`truncate px-1 hover:underline ${
                    i === crumbs.length - 1 ? "font-medium" : "text-text-secondary"
                  }`}
                >
                  {c.label}
                </button>
              </span>
            ))
          )}
        </nav>

        {/* Search (FR-BRW-3) */}
        {browsable && (
          <input
            value={browse.search}
            onChange={(e) => browse.setSearch(e.target.value)}
            placeholder="Search"
            aria-label="Search library"
            className="w-40 border border-border bg-bg px-2 py-1 text-[13px] placeholder:text-text-secondary focus:border-text focus:outline-none"
          />
        )}

        {/* View mode toggle */}
        <div className="flex border border-border" role="group" aria-label="View mode">
          <button
            title="List view"
            aria-pressed={browse.viewMode === "list"}
            onClick={() => browse.setViewMode("list")}
            className={`p-1.5 ${browse.viewMode === "list" ? "bg-accent text-accent-foreground" : "text-text-secondary hover:text-text"}`}
          >
            <ListIcon />
          </button>
          <button
            title="Icon view"
            aria-pressed={browse.viewMode === "grid"}
            onClick={() => browse.setViewMode("grid")}
            className={`p-1.5 ${browse.viewMode === "grid" ? "bg-accent text-accent-foreground" : "text-text-secondary hover:text-text"}`}
          >
            <GridIcon />
          </button>
        </div>

        {/* Look at this folder: view mode + reload */}
        <button
          title="Refresh library"
          disabled={!connected}
          onClick={() => void app.refreshEntries(true)}
          className={toolbarButton}
        >
          <RefreshIcon className={app.entriesLoading ? "animate-spin" : ""} />
        </button>

        <div className="mx-2 h-5 w-px bg-border" />

        {/* This folder: create, then the in/out pair */}
        <div className="flex items-center gap-0.5" role="group" aria-label="Folder">
          <button
            title="New folder"
            disabled={!browsable}
            onClick={() => browse.setNewFolderOpen(true)}
            className={toolbarButton}
          >
            <FolderPlusIcon />
          </button>
          <button
            title="Upload to current folder"
            disabled={!browsable}
            onClick={(e) => setUploadMenu({ x: e.clientX, y: e.clientY })}
            className={toolbarButton}
          >
            <UploadIcon />
          </button>
          <button
            title={
              browse.selection.length === 0
                ? "Select files or folders to download"
                : "Download selected to…"
            }
            disabled={!browsable || browse.selection.length === 0}
            onClick={() => void browse.downloadEntries(browse.selection)}
            className={toolbarButton}
          >
            <DownloadIcon />
          </button>
          <button
            title={
              browse.selection.length === 0
                ? "Select files or folders to delete"
                : "Delete selected…"
            }
            disabled={!browsable || browse.selection.length === 0}
            onClick={() => browse.setDeleteIds(browse.selection)}
            className={toolbarButton}
          >
            <TrashIcon />
          </button>
        </div>

        <div className="mx-2 h-5 w-px bg-border" />

        {/* Device and app */}
        <div className="flex items-center gap-0.5" role="group" aria-label="Device">
          <button
            title={syncPairCount === 0 ? "Set up folder sync" : "Sync now"}
            disabled={!connected && syncPairCount > 0}
            onClick={syncNow}
            className={toolbarButton}
          >
            <SyncIcon className={syncRunning ? "animate-spin" : ""} />
          </button>
          <button
            title="Connect to device"
            onClick={() => setDialog("connect")}
            className={toolbarButton}
          >
            <ConnectIcon />
          </button>
          <button
            title="Settings"
            onClick={() => openSettings("application")}
            className={toolbarButton}
          >
            <SettingsIcon />
          </button>
        </div>

        <div className="mx-2 h-5 w-px bg-border" />

        {/* Activity tray: last, like a badgeable inbox */}
        <button
          title="Transfers"
          onClick={() => setTransfersOpen((v) => !v)}
          className={`${toolbarButton} relative`}
        >
          <TransfersIcon />
          {activeTransfers.length > 0 && (
            <span className="absolute -right-0.5 -top-0.5 min-w-4 border border-border bg-bg px-0.5 text-center text-[10px] leading-4 tabular-nums">
              {activeTransfers.length}
            </span>
          )}
        </button>
      </header>

      <div className="flex min-h-0 flex-1">
        {/* Sidebar: content views only */}
        <aside className="w-40 shrink-0 border-r border-border bg-surface p-2">
          <nav className="flex flex-col gap-0.5">
            {VIEWS.map(({ id, label, Icon }) => (
              <button
                key={id}
                onClick={() => switchView(id)}
                className={`flex items-center gap-2 px-2 py-1.5 text-left text-[13px] ${
                  id === activeView
                    ? "bg-accent text-accent-foreground"
                    : "text-text-secondary hover:bg-bg hover:text-text"
                }`}
              >
                <Icon />
                {label}
              </button>
            ))}
          </nav>
        </aside>

        {/* Main content */}
        <main className="flex min-w-0 flex-1 flex-col">
          {!connected ? (
            <EmptyState
              connectionState={app.connection.state}
              onConnect={() => setDialog("connect")}
            />
          ) : view.root ? (
            <LibraryView />
          ) : (
            <div className="flex flex-1 items-center justify-center text-center text-text-secondary">
              <div>
                <h1 className="mb-1 text-xl font-semibold text-text">Templates</h1>
                <p>Template management arrives in a later release.</p>
              </div>
            </div>
          )}
        </main>
      </div>

      {/* Status bar */}
      <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-surface px-3 text-xs text-text-secondary">
        <span className="inline-flex items-center gap-1.5">
          {/* Monochrome status: filled dot = connected, hollow = not */}
          <span
            className={`size-2 border border-text ${connected ? "bg-text" : "bg-transparent"}`}
            style={{ borderRadius: "50%" }}
          />
          {connectionLabel[app.connection.state]}
        </span>
        {activeTransfers.length > 0 && (
          <button
            onClick={() => setTransfersOpen(true)}
            className="inline-flex items-center gap-2 hover:text-text"
          >
            <span>
              {runningJob
                ? `${runningJob.kind === "upload" ? "Uploading" : "Downloading"} “${runningJob.name}”`
                : "Transfers queued"}
              {activeTransfers.length > 1
                ? ` (+${activeTransfers.length - 1} queued)`
                : ""}
            </span>
            {runningJob?.progress != null && (
              <span className="inline-block h-1 w-24 border border-border align-middle">
                <span
                  className="block h-full bg-text"
                  style={{ width: `${Math.round(runningJob.progress * 100)}%` }}
                />
              </span>
            )}
          </button>
        )}
        {(syncRunning || (syncPairCount > 0 && lastSync)) && (
          <button
            onClick={() => openSettings("sync")}
            className="inline-flex items-center gap-2 hover:text-text"
          >
            {syncRunning ? (
              <>
                <span>
                  {syncRunning.phase === "apply" && syncRunning.total > 0
                    ? `Sync: ${syncRunning.done}/${syncRunning.total}`
                    : "Sync: preparing…"}
                </span>
                {syncRunning.total > 0 && (
                  <span className="inline-block h-1 w-24 border border-border align-middle">
                    <span
                      className="block h-full bg-text"
                      style={{
                        width: `${Math.round((syncRunning.done / syncRunning.total) * 100)}%`,
                      }}
                    />
                  </span>
                )}
              </>
            ) : (
              <span>
                Sync: {lastSync!.result === "ok" ? "up to date" : lastSync!.result} ·{" "}
                {new Date(lastSync!.finished_at).toLocaleTimeString()}
              </span>
            )}
          </button>
        )}
        <span className="ml-auto tabular-nums">v{app.version || "…"}</span>
      </footer>

      {/* Panels, dialogs, menus, toasts */}
      {transfersOpen && <TransfersPanel onClose={() => setTransfersOpen(false)} />}
      {dialog === "connect" && <ConnectDialog onClose={() => setDialog(null)} />}
      {dialog === "settings" && (
        <SettingsDialog onClose={() => setDialog(null)} initialTab={settingsTab} />
      )}
      {app.syncConfirmation && <SyncConfirmDialog request={app.syncConfirmation} />}
      {uploadMenu && (
        <ContextMenu
          x={uploadMenu.x}
          y={uploadMenu.y}
          items={[
            { label: "Upload files…", onClick: () => void browse.pickAndUploadFiles() },
            { label: "Upload folder…", onClick: () => void browse.pickAndUploadFolder() },
          ]}
          onClose={() => setUploadMenu(null)}
        />
      )}
      <Toasts />
    </div>
  );
}

function EmptyState({
  connectionState,
  onConnect,
}: {
  connectionState: string;
  onConnect: () => void;
}) {
  return (
    <div className="flex flex-1 items-center justify-center">
      <div className="max-w-sm text-center">
        <h1 className="mb-2 text-xl font-semibold">
          {connectionState === "connecting" ? "Connecting…" : "No device connected"}
        </h1>
        <p className="mb-4 text-text-secondary">
          Switch on Wi-Fi on your Digital Paper, make sure it is on the same network, then
          connect or pair it here.
        </p>
        <button
          onClick={onConnect}
          className="border border-accent bg-accent px-4 py-2 text-accent-foreground"
        >
          Connect to device
        </button>
      </div>
    </div>
  );
}
