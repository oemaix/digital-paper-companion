import { useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import Dialog from "./Dialog";
import IconButton from "./IconButton";
import {
  DownloadIcon,
  EyeIcon,
  HistoryIcon,
  PencilIcon,
  PlayIcon,
  StopIcon,
  SyncIcon,
  TrashIcon,
  UploadIcon,
} from "./icons";
import { useApp } from "../lib/store";
import {
  ipc,
  errorMessage,
  type SyncMode,
  type SyncPair,
  type SyncPairInfo,
  type SyncPlan,
  type SyncAction,
  type SyncRunRecord,
  type ExcludedSyncAction,
} from "../lib/ipc";
import { t as tt, useT } from "../lib/i18n";
import { currentLocale } from "../lib/i18n";

/**
 * Sync tab of the settings dialog (docs/05 §3.6; FR-SYN-1…8): the pair
 * list with per-pair actions, the inline pair editor, the dry-run preview
 * dialog and the run history.
 */

function modeLabel(mode: SyncMode): string {
  switch (mode) {
    case "two-way":
      return tt("sync.modeTwoWay");
    case "mirror-to-local":
      return tt("sync.modeMirrorLocal");
    case "mirror-to-remote":
      return tt("sync.modeMirrorRemote");
  }
}

function emptyPair(): SyncPair {
  return {
    id: "",
    name: "",
    local_root: "",
    remote_root: "Document",
    mode: "two-way",
    on_connect: false,
    interval_minutes: null,
    deletion_threshold: 10,
    filters: [],
    enabled: true,
  };
}

function scheduleSummary(pair: SyncPair): string {
  const parts: string[] = [];
  if (pair.on_connect) parts.push(tt("sync.onConnect"));
  if (pair.interval_minutes) {
    parts.push(tt("sync.everyN", { n: pair.interval_minutes }));
  }
  return parts.length > 0 ? parts.join(" · ") : tt("sync.manualOnly");
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? iso : d.toLocaleString(currentLocale());
}

/** Direction icon for a sync mode; the mode name lives in the tooltip. */
function ModeIcon({ mode }: { mode: SyncMode }) {
  const label = modeLabel(mode);
  const Icon =
    mode === "two-way"
      ? SyncIcon
      : mode === "mirror-to-remote"
        ? UploadIcon
        : DownloadIcon;
  return (
    <span title={label} className="shrink-0">
      <Icon width={13} height={13} aria-label={label} />
    </span>
  );
}

/**
 * A path that, when space runs out, is ellipsized in the middle: the start
 * shrinks with `…` while the tail (the most telling part) stays visible.
 */
function MiddleTruncatedPath({ path }: { path: string }) {
  const TAIL = 12;
  if (path.length <= TAIL + 4) {
    return (
      <span className="truncate" title={path}>
        {path}
      </span>
    );
  }
  return (
    <span className="flex min-w-0" title={path}>
      <span className="truncate whitespace-pre">{path.slice(0, -TAIL)}</span>
      <span className="shrink-0 whitespace-pre">{path.slice(-TAIL)}</span>
    </span>
  );
}

function resultLabel(result: string): string {
  switch (result) {
    case "ok":
      return tt("sync.resultOk");
    case "partial":
      return tt("sync.resultPartial");
    case "cancelled":
      return tt("sync.resultCancelled");
    case "failed":
      return tt("sync.resultFailed");
    default:
      return result;
  }
}

export default function SyncSettings({
  addPairRequest = 0,
}: {
  /** Incremented by the host (the "+" in the settings tab row) to open the editor. */
  addPairRequest?: number;
}) {
  const t = useT();
  const app = useApp();
  const pairs = app.syncPairs ?? [];
  const connected = app.connection.state === "connected";
  const [editing, setEditing] = useState<SyncPair | null>(null);
  const [historyFor, setHistoryFor] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ pairId: string; plan: SyncPlan } | null>(null);

  useEffect(() => {
    if (addPairRequest > 0) setEditing(emptyPair());
  }, [addPairRequest]);

  const runNow = (id: string) => {
    void ipc.syncRun(id).then(
      () => app.toast(t("sync.queuedToast")),
      (err) => app.toast(errorMessage(err), "error"),
    );
  };

  const openPreview = async (id: string) => {
    setPreviewLoading(id);
    try {
      const plan = await ipc.syncPreview(id);
      setPreview({ pairId: id, plan });
    } catch (err) {
      app.toast(errorMessage(err), "error");
    } finally {
      setPreviewLoading(null);
    }
  };

  const deletePair = async (pair: SyncPairInfo) => {
    if (
      !window.confirm(t("sync.removeConfirm", { name: pair.name || pair.remote_root }))
    ) {
      return;
    }
    try {
      await ipc.syncPairDelete(pair.id);
      await app.refreshSyncPairs();
    } catch (err) {
      app.toast(errorMessage(err), "error");
    }
  };

  return (
    <div className="flex flex-col gap-3 text-[13px]">
      {pairs.length === 0 && (
        <>
          <p className="text-text-secondary">{t("sync.intro")}</p>
          <button
            onClick={() => setEditing(emptyPair())}
            className="self-start border border-border px-3 py-1.5 hover:border-text"
          >
            {t("sync.addPair")}
          </button>
        </>
      )}

      {pairs.map((pair) => {
        const running = app.syncStatus.running?.pair_id === pair.id;
        const queued = app.syncStatus.queued.includes(pair.id);
        return (
          <div key={pair.id}>
            <div className="flex items-center gap-2">
              <span
                className="min-w-0 truncate font-medium"
                title={pair.name || pair.remote_root}
              >
                {pair.name || pair.remote_root}
              </span>
              {/* With a custom name, keep the device folder visible next to it. */}
              {pair.name && (
                <span className="flex min-w-0 shrink-[2] text-xs text-text-secondary">
                  <MiddleTruncatedPath path={pair.remote_root} />
                </span>
              )}
              {!pair.enabled && (
                <span className="shrink-0 border border-border px-1 text-[11px] text-text-secondary">
                  {t("sync.disabledBadge")}
                </span>
              )}
              <span className="min-w-0 flex-1" />
              <div className="flex shrink-0 items-center gap-0.5">
                {running || queued ? (
                  <IconButton
                    title={t("common.cancel")}
                    onClick={() => void ipc.syncCancel(pair.id)}
                  >
                    <StopIcon width={14} height={14} />
                  </IconButton>
                ) : (
                  <>
                    <IconButton
                      title={t("sync.syncNow")}
                      disabled={!connected}
                      onClick={() => runNow(pair.id)}
                    >
                      <PlayIcon width={14} height={14} />
                    </IconButton>
                    <IconButton
                      title={
                        previewLoading === pair.id
                          ? t("sync.planning")
                          : t("sync.preview")
                      }
                      disabled={!connected || previewLoading === pair.id}
                      onClick={() => void openPreview(pair.id)}
                    >
                      <EyeIcon
                        width={14}
                        height={14}
                        className={previewLoading === pair.id ? "animate-pulse" : ""}
                      />
                    </IconButton>
                  </>
                )}
                <IconButton
                  title={t("common.edit")}
                  onClick={() => setEditing({ ...pair })}
                >
                  <PencilIcon width={14} height={14} />
                </IconButton>
                <IconButton
                  title={t("sync.history")}
                  onClick={() => setHistoryFor(historyFor === pair.id ? null : pair.id)}
                >
                  <HistoryIcon width={14} height={14} />
                </IconButton>
                <IconButton
                  title={t("common.remove")}
                  onClick={() => void deletePair(pair)}
                >
                  <TrashIcon width={14} height={14} />
                </IconButton>
              </div>
            </div>
            <div className="mt-0.5 flex items-center gap-1.5 text-xs text-text-secondary">
              <ModeIcon mode={pair.mode} />
              <MiddleTruncatedPath path={pair.local_root} />
              <span className="shrink-0">· {scheduleSummary(pair)}</span>
            </div>
            <div className="mt-0.5 text-xs text-text-secondary">
              {running
                ? t("sync.running")
                : queued
                  ? t("sync.queuedState")
                  : pair.last_run
                    ? t("sync.lastRun", {
                        result: resultLabel(pair.last_run.result),
                        time: formatTime(pair.last_run.finished_at),
                      })
                    : t("sync.neverSynced")}
            </div>
            {historyFor === pair.id && <History pairId={pair.id} />}
          </div>
        );
      })}

      {editing && (
        <PairEditor
          initial={editing}
          onDone={async () => {
            setEditing(null);
            await app.refreshSyncPairs();
          }}
          onCancel={() => setEditing(null)}
        />
      )}

      {preview && (
        <PreviewDialog
          pairId={preview.pairId}
          plan={preview.plan}
          onClose={() => setPreview(null)}
          onApplied={() => {
            setPreview(null);
            app.toast(t("sync.queuedToast"));
          }}
        />
      )}
    </div>
  );
}

// ---- pair editor ---------------------------------------------------------------

function PairEditor({
  initial,
  onDone,
  onCancel,
}: {
  initial: SyncPair;
  onDone: () => Promise<void>;
  onCancel: () => void;
}) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [pair, setPair] = useState<SyncPair>(initial);
  const [filtersText, setFiltersText] = useState(initial.filters.join("\n"));
  const [saving, setSaving] = useState(false);

  const pickFolder = async () => {
    const selected = await openFileDialog({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setPair((p) => ({ ...p, local_root: selected }));
    }
  };

  const save = async () => {
    setSaving(true);
    try {
      await ipc.syncPairUpsert({
        ...pair,
        filters: filtersText
          .split("\n")
          .map((l) => l.trim())
          .filter(Boolean),
        interval_minutes: pair.interval_minutes || null,
      });
      await onDone();
    } catch (err) {
      toast(errorMessage(err), "error");
    } finally {
      setSaving(false);
    }
  };

  const field =
    "border border-border bg-bg px-2 py-1 focus:border-text focus:outline-none";

  return (
    <Dialog
      title={initial.id ? t("sync.editPair") : t("sync.addPair")}
      onClose={onCancel}
    >
      <div className="flex flex-col gap-3 text-[13px]">
        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("sync.editorName")}</span>
          <input
            value={pair.name}
            onChange={(e) => setPair({ ...pair, name: e.target.value })}
            placeholder={t("sync.editorNamePlaceholder")}
            className={field}
          />
        </label>

        <div className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("sync.editorLocalFolder")}</span>
          <div className="flex gap-1.5">
            <input
              value={pair.local_root}
              onChange={(e) => setPair({ ...pair, local_root: e.target.value })}
              placeholder="/home/…/DigitalPaper"
              aria-label={t("sync.editorLocalFolder")}
              className={`${field} min-w-0 flex-1`}
            />
            <button
              onClick={() => void pickFolder()}
              className="border border-border px-2 hover:border-text"
            >
              {t("sync.editorChoose")}
            </button>
          </div>
        </div>

        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("sync.editorDeviceFolder")}</span>
          <input
            value={pair.remote_root}
            onChange={(e) => setPair({ ...pair, remote_root: e.target.value })}
            placeholder="Document"
            className={field}
          />
        </label>

        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("sync.editorMode")}</span>
          <select
            value={pair.mode}
            onChange={(e) => setPair({ ...pair, mode: e.target.value as SyncMode })}
            className={field}
          >
            <option value="two-way">{t("sync.editorModeTwoWay")}</option>
            <option value="mirror-to-local">{t("sync.editorModeMirrorLocal")}</option>
            <option value="mirror-to-remote">{t("sync.editorModeMirrorRemote")}</option>
          </select>
        </label>

        <fieldset className="flex flex-col gap-1.5">
          <span className="text-text-secondary">{t("sync.editorSchedule")}</span>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={pair.on_connect}
              onChange={(e) => setPair({ ...pair, on_connect: e.target.checked })}
            />
            {t("sync.editorOnConnect")}
          </label>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={!!pair.interval_minutes}
              onChange={(e) =>
                setPair({ ...pair, interval_minutes: e.target.checked ? 30 : null })
              }
            />
            {t("sync.editorEveryPre")}
            <input
              type="number"
              min={1}
              max={1440}
              value={pair.interval_minutes ?? 30}
              disabled={!pair.interval_minutes}
              onChange={(e) =>
                setPair({
                  ...pair,
                  interval_minutes: Math.max(1, Number(e.target.value) || 1),
                })
              }
              className={`${field} w-16 disabled:opacity-40`}
            />
            {t("sync.editorEveryPost")}
          </label>
        </fieldset>

        <label className="flex flex-col gap-1">
          <span className="text-text-secondary">{t("sync.editorFilters")}</span>
          <textarea
            value={filtersText}
            onChange={(e) => setFiltersText(e.target.value)}
            rows={2}
            className={`${field} font-mono text-xs`}
          />
        </label>

        <label className="flex items-center justify-between gap-4">
          <span>
            {t("sync.editorThresholdPre")}
            <input
              type="number"
              min={0}
              value={pair.deletion_threshold}
              onChange={(e) =>
                setPair({
                  ...pair,
                  deletion_threshold: Math.max(0, Number(e.target.value) || 0),
                })
              }
              className={`${field} mx-1.5 w-16`}
            />
            {t("sync.editorThresholdPost")}
          </span>
        </label>

        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={pair.enabled}
            onChange={(e) => setPair({ ...pair, enabled: e.target.checked })}
          />
          {t("sync.editorEnabled")}
        </label>

        <div className="flex justify-end gap-2 border-t border-border pt-3">
          <button
            onClick={onCancel}
            className="border border-border px-3 py-1.5 hover:border-text"
          >
            {t("common.cancel")}
          </button>
          <button
            onClick={() => void save()}
            disabled={saving || !pair.local_root.trim()}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
          >
            {saving ? t("common.saving") : t("common.save")}
          </button>
        </div>
      </div>
    </Dialog>
  );
}

// ---- run history (FR-SYN-7) ------------------------------------------------------

function History({ pairId }: { pairId: string }) {
  const t = useT();
  const [records, setRecords] = useState<SyncRunRecord[] | null>(null);

  useEffect(() => {
    void ipc.syncHistory(pairId).then(setRecords, () => setRecords([]));
  }, [pairId]);

  // A left rule ties the expanded history to the pair above (no outer box).
  const block = "mt-1.5 border-l-2 border-border pl-2.5 text-xs";

  if (records === null) {
    return (
      <p className={`${block} py-1 text-text-secondary`}>{t("sync.historyLoading")}</p>
    );
  }
  if (records.length === 0) {
    return (
      <p className={`${block} py-1 text-text-secondary`}>{t("sync.historyEmpty")}</p>
    );
  }
  return (
    <ul className={`${block} max-h-48 overflow-auto`}>
      {records.map((r, i) => (
        <li
          key={i}
          className="border-b border-border py-1.5 first:pt-0.5 last:border-b-0"
        >
          <div className="flex justify-between gap-2">
            <span className="font-medium">{resultLabel(r.result)}</span>
            <span className="text-text-secondary">
              {formatTime(r.finished_at)} · {r.trigger}
              {r.device_serial ? ` · ${r.device_serial}` : ""}
            </span>
          </div>
          <div className="text-text-secondary">
            {t("sync.historyDone", { n: r.done })}
            {r.failed > 0 ? t("sync.historyFailed", { n: r.failed }) : ""}
            {r.skipped > 0 ? t("sync.historySkipped", { n: r.skipped }) : ""}
            {r.conflicts.length > 0
              ? t("sync.historyConflicts", { n: r.conflicts.length })
              : ""}
          </div>
          {r.errors.slice(0, 3).map((e, j) => (
            <div key={j} className="truncate text-text-secondary" title={e}>
              ⚠ {e}
            </div>
          ))}
        </li>
      ))}
    </ul>
  );
}

// ---- preview / dry-run dialog (FR-SYN-5) ------------------------------------------

const SELECTABLE = new Set([
  "upload",
  "download",
  "conflict_resolve",
  "delete_local",
  "delete_remote",
  "delete_local_dir",
  "delete_remote_dir",
]);

function actionKey(a: SyncAction): string {
  return `${a.kind}:${a.relpath}`;
}

export function PreviewDialog({
  pairId,
  plan,
  onClose,
  onApplied,
}: {
  pairId: string;
  plan: SyncPlan;
  onClose: () => void;
  onApplied: () => void;
}) {
  const t = useT();
  const toast = useApp((s) => s.toast);
  const [deselected, setDeselected] = useState<Set<string>>(new Set());

  const groups = useMemo(() => {
    const uploads = plan.actions.filter((a) => a.kind === "upload");
    const downloads = plan.actions.filter((a) => a.kind === "download");
    const conflicts = plan.actions.filter((a) => a.kind === "conflict_resolve");
    const deletions = plan.actions.filter((a) => a.kind.startsWith("delete_"));
    return { uploads, downloads, conflicts, deletions };
  }, [plan]);

  const selectable = plan.actions.filter((a) => SELECTABLE.has(a.kind));
  const nothing =
    selectable.length === 0 &&
    plan.summary.create_local_dirs === 0 &&
    plan.summary.create_remote_dirs === 0 &&
    plan.summary.adopts === 0;

  const toggle = (a: SyncAction) => {
    const key = actionKey(a);
    setDeselected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const apply = async () => {
    const excluded: ExcludedSyncAction[] = selectable
      .filter((a) => deselected.has(actionKey(a)))
      .map((a) => ({ kind: a.kind, relpath: a.relpath }));
    try {
      await ipc.syncRun(pairId, true, excluded);
      onApplied();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const column = (
    title: string,
    actions: SyncAction[],
    describe?: (a: SyncAction) => string,
  ) => (
    <div className="min-w-0 flex-1">
      <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
        {title} ({actions.length})
      </h3>
      <ul className="max-h-64 overflow-auto border border-border">
        {actions.length === 0 && (
          <li className="p-1.5 text-xs text-text-secondary">{t("common.none")}</li>
        )}
        {actions.map((a) => (
          <li key={actionKey(a)} className="border-b border-border last:border-b-0">
            <label className="flex items-start gap-1.5 p-1.5 text-xs">
              <input
                type="checkbox"
                checked={!deselected.has(actionKey(a))}
                onChange={() => toggle(a)}
                className="mt-0.5"
              />
              <span className="min-w-0">
                <span className="block truncate" title={a.relpath}>
                  {a.relpath}
                </span>
                {describe && <span className="text-text-secondary">{describe(a)}</span>}
              </span>
            </label>
          </li>
        ))}
      </ul>
    </div>
  );

  return (
    <Dialog title={t("syncPreview.title")} onClose={onClose} width="lg">
      {nothing ? (
        <p className="text-text-secondary">{t("syncPreview.nothing")}</p>
      ) : (
        <div className="flex flex-col gap-3 text-[13px]">
          {plan.warnings.length > 0 && (
            <div className="border border-border p-2 text-xs">
              {plan.warnings.map((w, i) => (
                <div key={i}>⚠ {w}</div>
              ))}
            </div>
          )}
          <div className="flex gap-3">
            {column(t("syncPreview.uploads"), [...groups.uploads])}
            {column(t("syncPreview.downloads"), groups.downloads)}
            {column(t("syncPreview.deletions"), groups.deletions, (a) =>
              a.kind.startsWith("delete_local")
                ? t("syncPreview.onComputer")
                : t("syncPreview.onDevice"),
            )}
          </div>
          {groups.conflicts.length > 0 &&
            column(t("syncPreview.conflicts"), groups.conflicts, (a) =>
              a.winner === "remote"
                ? t("syncPreview.deviceWins")
                : t("syncPreview.computerWins"),
            )}
        </div>
      )}
      <div className="mt-4 flex gap-2 border-t border-border pt-3">
        {!nothing && (
          <button
            onClick={() => void apply()}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground"
          >
            {t("common.apply")}
          </button>
        )}
        <button
          onClick={onClose}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {nothing ? t("common.close") : t("common.cancel")}
        </button>
      </div>
    </Dialog>
  );
}
