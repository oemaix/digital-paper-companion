import { useEffect, useMemo, useState } from "react";
import ConnectDialog from "./components/ConnectDialog";
import SettingsDialog, { type SettingsTab } from "./components/SettingsDialog";
import SyncConfirmDialog from "./components/SyncConfirmDialog";
import LibraryView from "./components/LibraryView";
import TemplatesView from "./components/TemplatesView";
import TransfersPanel from "./components/TransfersPanel";
import ContextMenu from "./components/ContextMenu";
import Toasts from "./components/Toasts";
import {
  CameraIcon,
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
  TabletIcon,
  TemplateIcon,
  TransfersIcon,
  TrashIcon,
  UploadIcon,
} from "./components/icons";
import { useApp } from "./lib/store";
import { useBrowse } from "./lib/browse";
import { ipc, errorMessage } from "./lib/ipc";
import { useI18n, useT, type MessageKey } from "./lib/i18n";

/**
 * Application shell per docs/05 §2:
 * - sidebar: device selector (multi-device) + content views
 *   (Library, Notes, Templates)
 * - toolbar: breadcrumb, search, view-mode toggle, action icons
 * - status bar: connection state, transfer summary, version
 */

const VIEWS = [
  { id: "library", labelKey: "view.library", Icon: FolderIcon, root: "Document" },
  { id: "notes", labelKey: "view.notes", Icon: NoteIcon, root: "Document/Note" },
  { id: "templates", labelKey: "view.templates", Icon: TemplateIcon, root: null },
] as const satisfies readonly {
  id: string;
  labelKey: MessageKey;
  Icon: React.ComponentType<React.SVGProps<SVGSVGElement>>;
  root: string | null;
}[];

type ViewId = (typeof VIEWS)[number]["id"];
type DialogId = "connect" | "settings" | null;

export default function App() {
  const t = useT();
  const app = useApp();
  const browse = useBrowse();
  const [activeView, setActiveView] = useState<ViewId>("library");
  const [dialog, setDialog] = useState<DialogId>(null);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("application");
  const [transfersOpen, setTransfersOpen] = useState(false);
  const [uploadMenu, setUploadMenu] = useState<{ x: number; y: number } | null>(null);
  const [screenshotBusy, setScreenshotBusy] = useState(false);

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

  // Breadcrumb segments relative to the view root. Notes is a flat view
  // (docs/05 §3.3), so it shows only its label. `t` is a stable reference,
  // so the memo must depend on the locale itself to refresh on change.
  const locale = useI18n((s) => s.locale);
  const crumbs = useMemo(() => {
    if (!view.root || view.id === "notes") return [];
    const rel = browse.path.slice(view.root.length).split("/").filter(Boolean);
    const out: { label: string; path: string }[] = [
      { label: t(view.labelKey), path: view.root },
    ];
    let acc: string = view.root;
    for (const seg of rel) {
      acc = `${acc}/${seg}`;
      out.push({ label: seg, path: acc });
    }
    return out;
  }, [view, browse.path, t, locale]);

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
      (n) => app.toast(n === 0 ? t("sync.noPairs") : t("sync.started", { n })),
      (err) => app.toast(errorMessage(err), "error"),
    );
  };

  // Screenshot straight to the clipboard (FR-SET-5). Capturing takes a few
  // seconds, so announce it immediately — silence reads as a dead button.
  const copyScreenshot = async () => {
    setScreenshotBusy(true);
    app.toast(t("device.screenshotCapturing"));
    try {
      await ipc.copyScreenshot();
      app.toast(t("device.screenshotCopied"));
    } catch (err) {
      app.toast(errorMessage(err), "error");
    } finally {
      setScreenshotBusy(false);
    }
  };

  const switchDevice = (serial: string) => {
    if (serial === app.connection.serial) return;
    void ipc.connectKnownDevice(serial).then(
      () => app.toast(t("connect.connected")),
      (err) => app.toast(errorMessage(err), "error"),
    );
  };

  const connectionLabel: Record<string, string> = {
    disconnected: t("status.notConnected"),
    connecting: t("status.connecting"),
    connected: app.connection.name ?? t("status.connected"),
    reauthenticating: t("status.reconnecting"),
  };

  const toolbarButton =
    "flex items-center gap-1.5 border border-transparent p-1.5 text-text-secondary hover:border-border hover:text-text disabled:opacity-40 disabled:hover:border-transparent";

  return (
    <div className="relative flex h-screen flex-col">
      {/* Toolbar */}
      <header className="flex h-10 shrink-0 items-center gap-1 border-b border-border bg-surface px-2">
        {/* Breadcrumb */}
        <nav
          aria-label={t(view.labelKey)}
          className="flex min-w-0 flex-1 items-center gap-0.5 text-[13px]"
        >
          {crumbs.length === 0 ? (
            <span className="px-1 font-medium">{t(view.labelKey)}</span>
          ) : (
            crumbs.map((c, i) => (
              <span key={c.path} className="flex min-w-0 items-center gap-0.5">
                {i > 0 && (
                  <ChevronRightIcon
                    className="shrink-0 text-text-secondary rtl:-scale-x-100"
                    width={12}
                    height={12}
                  />
                )}
                <button
                  onClick={() => browse.navigate(c.path)}
                  aria-current={i === crumbs.length - 1 ? "location" : undefined}
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
            placeholder={t("toolbar.search")}
            aria-label={t("toolbar.searchLibrary")}
            className="w-40 border border-border bg-bg px-2 py-1 text-[13px] placeholder:text-text-secondary focus:border-text focus:outline-none"
          />
        )}

        {/* View mode toggle */}
        <div
          className="flex border border-border"
          role="group"
          aria-label={t("toolbar.viewMode")}
        >
          <button
            title={t("toolbar.listView")}
            aria-label={t("toolbar.listView")}
            aria-pressed={browse.viewMode === "list"}
            onClick={() => browse.setViewMode("list")}
            className={`p-1.5 ${browse.viewMode === "list" ? "bg-accent text-accent-foreground" : "text-text-secondary hover:text-text"}`}
          >
            <ListIcon />
          </button>
          <button
            title={t("toolbar.iconView")}
            aria-label={t("toolbar.iconView")}
            aria-pressed={browse.viewMode === "grid"}
            onClick={() => browse.setViewMode("grid")}
            className={`p-1.5 ${browse.viewMode === "grid" ? "bg-accent text-accent-foreground" : "text-text-secondary hover:text-text"}`}
          >
            <GridIcon />
          </button>
        </div>

        {/* Look at this folder: view mode + reload */}
        <button
          title={t("toolbar.refresh")}
          aria-label={t("toolbar.refresh")}
          disabled={!connected}
          onClick={() => void app.refreshEntries(true)}
          className={toolbarButton}
        >
          <RefreshIcon className={app.entriesLoading ? "animate-spin" : ""} />
        </button>
        <button
          title={t("toolbar.screenshot")}
          aria-label={t("toolbar.screenshot")}
          disabled={!connected || screenshotBusy}
          onClick={() => void copyScreenshot()}
          className={toolbarButton}
        >
          <CameraIcon className={screenshotBusy ? "animate-pulse" : ""} />
        </button>

        <div className="mx-2 h-5 w-px bg-border" />

        {/* This folder: create, then the in/out pair */}
        <div
          className="flex items-center gap-0.5"
          role="group"
          aria-label={t("toolbar.folderGroup")}
        >
          <button
            title={t("toolbar.newFolder")}
            aria-label={t("toolbar.newFolder")}
            disabled={!browsable}
            onClick={() => browse.setNewFolderOpen(true)}
            className={toolbarButton}
          >
            <FolderPlusIcon />
          </button>
          <button
            title={t("toolbar.upload")}
            aria-label={t("toolbar.upload")}
            disabled={!browsable}
            onClick={(e) => setUploadMenu({ x: e.clientX, y: e.clientY })}
            className={toolbarButton}
          >
            <UploadIcon />
          </button>
          <button
            title={
              browse.selection.length === 0
                ? t("toolbar.downloadSelect")
                : t("toolbar.downloadSelected")
            }
            aria-label={t("toolbar.downloadSelected")}
            disabled={!browsable || browse.selection.length === 0}
            onClick={() => void browse.downloadEntries(browse.selection)}
            className={toolbarButton}
          >
            <DownloadIcon />
          </button>
          <button
            title={
              browse.selection.length === 0
                ? t("toolbar.deleteSelect")
                : t("toolbar.deleteSelected")
            }
            aria-label={t("toolbar.deleteSelected")}
            disabled={!browsable || browse.selection.length === 0}
            onClick={() => browse.setDeleteIds(browse.selection)}
            className={toolbarButton}
          >
            <TrashIcon />
          </button>
        </div>

        <div className="mx-2 h-5 w-px bg-border" />

        {/* Device and app */}
        <div
          className="flex items-center gap-0.5"
          role="group"
          aria-label={t("toolbar.deviceGroup")}
        >
          <button
            title={syncPairCount === 0 ? t("toolbar.syncSetup") : t("toolbar.syncNow")}
            aria-label={t("toolbar.syncNow")}
            disabled={!connected && syncPairCount > 0}
            onClick={syncNow}
            className={toolbarButton}
          >
            <SyncIcon className={syncRunning ? "animate-spin" : ""} />
          </button>
          <button
            title={t("toolbar.connect")}
            aria-label={t("toolbar.connect")}
            onClick={() => setDialog("connect")}
            className={toolbarButton}
          >
            <ConnectIcon />
          </button>
          <button
            title={t("toolbar.settings")}
            aria-label={t("toolbar.settings")}
            onClick={() => openSettings("application")}
            className={toolbarButton}
          >
            <SettingsIcon />
          </button>
        </div>

        <div className="mx-2 h-5 w-px bg-border" />

        {/* Activity tray: last, like a badgeable inbox */}
        <button
          title={t("toolbar.transfers")}
          aria-label={t("toolbar.transfers")}
          onClick={() => setTransfersOpen((v) => !v)}
          className={`${toolbarButton} relative`}
        >
          <TransfersIcon />
          {activeTransfers.length > 0 && (
            <span className="absolute -end-0.5 -top-0.5 min-w-4 border border-border bg-bg px-0.5 text-center text-[10px] leading-4 tabular-nums">
              {activeTransfers.length}
            </span>
          )}
        </button>
      </header>

      <div className="flex min-h-0 flex-1">
        {/* Sidebar: device selector (FR-CONN-7) + content views */}
        <aside className="flex w-40 shrink-0 flex-col border-r border-border bg-surface p-2">
          {app.knownDevices.length > 1 && (
            <label className="mb-2 flex flex-col gap-1">
              <span className="flex items-center gap-1.5 px-1 text-[11px] uppercase tracking-wide text-text-secondary">
                <TabletIcon width={12} height={12} />
                {t("sidebar.device")}
              </span>
              <select
                value={app.connection.serial ?? ""}
                onChange={(e) => e.target.value && switchDevice(e.target.value)}
                aria-label={t("sidebar.switchDevice")}
                className="w-full border border-border bg-bg px-1.5 py-1 text-xs focus:border-text focus:outline-none"
              >
                {!app.connection.serial && <option value="">—</option>}
                {app.knownDevices.map((d) => (
                  <option key={d.serial} value={d.serial}>
                    {d.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <nav className="flex flex-col gap-0.5" aria-label={t("sidebar.device")}>
            {VIEWS.map(({ id, labelKey, Icon }) => (
              <button
                key={id}
                onClick={() => switchView(id)}
                aria-current={id === activeView ? "page" : undefined}
                className={`flex items-center gap-2 px-2 py-1.5 text-start text-[13px] ${
                  id === activeView
                    ? "bg-accent text-accent-foreground"
                    : "text-text-secondary hover:bg-bg hover:text-text"
                }`}
              >
                <Icon />
                {t(labelKey)}
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
            <LibraryView notes={view.id === "notes"} />
          ) : (
            <TemplatesView />
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
                ? runningJob.kind === "download"
                  ? t("status.downloading", { name: runningJob.name })
                  : t("status.uploading", { name: runningJob.name })
                : t("status.transfersQueued")}
              {activeTransfers.length > 1
                ? t("status.moreQueued", { n: activeTransfers.length - 1 })
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
                    ? t("status.syncProgress", {
                        done: syncRunning.done,
                        total: syncRunning.total,
                      })
                    : t("status.syncPreparing")}
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
                {lastSync!.result === "ok"
                  ? t("status.syncUpToDate")
                  : t("status.syncResult", { result: lastSync!.result })}
                {" · "}
                {new Date(lastSync!.finished_at).toLocaleTimeString()}
              </span>
            )}
          </button>
        )}
        <span className="ms-auto tabular-nums">v{app.version || "…"}</span>
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
            {
              label: t("toolbar.uploadFiles"),
              onClick: () => void browse.pickAndUploadFiles(),
            },
            {
              label: t("toolbar.uploadFolder"),
              onClick: () => void browse.pickAndUploadFolder(),
            },
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
  const t = useT();
  return (
    <div className="flex flex-1 items-center justify-center">
      <div className="max-w-sm text-center">
        <h1 className="mb-2 text-xl font-semibold">
          {connectionState === "connecting" ? t("empty.connecting") : t("empty.noDevice")}
        </h1>
        <p className="mb-4 text-text-secondary">{t("empty.hint")}</p>
        <button
          onClick={onConnect}
          className="border border-accent bg-accent px-4 py-2 text-accent-foreground"
        >
          {t("empty.connectButton")}
        </button>
      </div>
    </div>
  );
}
