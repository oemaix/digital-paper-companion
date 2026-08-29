import { useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import Dialog from "./Dialog";
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

/**
 * Sync tab of the settings dialog (docs/05 §3.6; FR-SYN-1…8): the pair
 * list with per-pair actions, the inline pair editor, the dry-run preview
 * dialog and the run history.
 */

const MODE_LABEL: Record<SyncMode, string> = {
  "two-way": "Two-way",
  "mirror-to-local": "Mirror to computer",
  "mirror-to-remote": "Mirror to device",
};

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
  if (pair.on_connect) parts.push("on connect");
  if (pair.interval_minutes) parts.push(`every ${pair.interval_minutes} min`);
  return parts.length > 0 ? parts.join(" · ") : "manual only";
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  return isNaN(d.getTime()) ? iso : d.toLocaleString();
}

const RESULT_LABEL: Record<string, string> = {
  ok: "OK",
  partial: "Partial",
  cancelled: "Cancelled",
  failed: "Failed",
};

export default function SyncSettings() {
  const app = useApp();
  const pairs = app.syncPairs ?? [];
  const connected = app.connection.state === "connected";
  const [editing, setEditing] = useState<SyncPair | null>(null);
  const [historyFor, setHistoryFor] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ pairId: string; plan: SyncPlan } | null>(null);

  const runNow = (id: string) => {
    void ipc.syncRun(id).then(
      () => app.toast("Sync queued"),
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
      !window.confirm(
        `Remove sync pair “${pair.name || pair.local_root}”? Local files are not touched.`,
      )
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

  if (editing) {
    return (
      <PairEditor
        initial={editing}
        onDone={async () => {
          setEditing(null);
          await app.refreshSyncPairs();
        }}
        onCancel={() => setEditing(null)}
      />
    );
  }

  return (
    <div className="flex flex-col gap-3">
      {pairs.length === 0 && (
        <p className="text-text-secondary">
          A sync pair keeps a local folder and a device folder in step — two-way or as a
          mirror. Runs can be scheduled on connect or on an interval.
        </p>
      )}

      {pairs.map((pair) => {
        const running = app.syncStatus.running?.pair_id === pair.id;
        const queued = app.syncStatus.queued.includes(pair.id);
        return (
          <div key={pair.id} className="border border-border">
            <div className="flex items-start gap-2 p-2.5">
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">
                    {pair.name || pair.local_root}
                  </span>
                  {!pair.enabled && (
                    <span className="border border-border px-1 text-[11px] text-text-secondary">
                      disabled
                    </span>
                  )}
                </div>
                <div className="mt-0.5 truncate text-xs text-text-secondary">
                  {pair.local_root} ⇄ {pair.remote_root} · {MODE_LABEL[pair.mode]} ·{" "}
                  {scheduleSummary(pair)}
                </div>
                <div className="mt-0.5 text-xs text-text-secondary">
                  {running
                    ? "Running…"
                    : queued
                      ? "Queued"
                      : pair.last_run
                        ? `Last run ${RESULT_LABEL[pair.last_run.result] ?? pair.last_run.result} · ${formatTime(pair.last_run.finished_at)}`
                        : "Never synced"}
                </div>
              </div>
            </div>
            <div className="flex gap-1 border-t border-border px-2 py-1.5 text-xs">
              {running || queued ? (
                <button
                  onClick={() => void ipc.syncCancel(pair.id)}
                  className="border border-border px-2 py-1 hover:border-text"
                >
                  Cancel
                </button>
              ) : (
                <>
                  <button
                    onClick={() => runNow(pair.id)}
                    disabled={!connected}
                    className="border border-border px-2 py-1 hover:border-text disabled:opacity-40"
                  >
                    Sync now
                  </button>
                  <button
                    onClick={() => void openPreview(pair.id)}
                    disabled={!connected || previewLoading === pair.id}
                    className="border border-border px-2 py-1 hover:border-text disabled:opacity-40"
                  >
                    {previewLoading === pair.id ? "Planning…" : "Preview"}
                  </button>
                </>
              )}
              <button
                onClick={() => setEditing({ ...pair })}
                className="border border-border px-2 py-1 hover:border-text"
              >
                Edit
              </button>
              <button
                onClick={() => setHistoryFor(historyFor === pair.id ? null : pair.id)}
                className="border border-border px-2 py-1 hover:border-text"
              >
                History
              </button>
              <button
                onClick={() => void deletePair(pair)}
                className="ml-auto border border-transparent px-2 py-1 text-text-secondary hover:border-border hover:text-text"
              >
                Remove
              </button>
            </div>
            {historyFor === pair.id && <History pairId={pair.id} />}
          </div>
        );
      })}

      <button
        onClick={() => setEditing(emptyPair())}
        className="self-start border border-border px-3 py-1.5 hover:border-text"
      >
        Add sync pair
      </button>

      {preview && (
        <PreviewDialog
          pairId={preview.pairId}
          plan={preview.plan}
          onClose={() => setPreview(null)}
          onApplied={() => {
            setPreview(null);
            app.toast("Sync queued");
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
    <div className="flex flex-col gap-3 text-[13px]">
      <label className="flex flex-col gap-1">
        <span className="text-text-secondary">Name (optional)</span>
        <input
          value={pair.name}
          onChange={(e) => setPair({ ...pair, name: e.target.value })}
          placeholder="e.g. Papers"
          className={field}
        />
      </label>

      <div className="flex flex-col gap-1">
        <span className="text-text-secondary">Local folder</span>
        <div className="flex gap-1.5">
          <input
            value={pair.local_root}
            onChange={(e) => setPair({ ...pair, local_root: e.target.value })}
            placeholder="/home/…/DigitalPaper"
            className={`${field} min-w-0 flex-1`}
          />
          <button
            onClick={() => void pickFolder()}
            className="border border-border px-2 hover:border-text"
          >
            Choose…
          </button>
        </div>
      </div>

      <label className="flex flex-col gap-1">
        <span className="text-text-secondary">Device folder</span>
        <input
          value={pair.remote_root}
          onChange={(e) => setPair({ ...pair, remote_root: e.target.value })}
          placeholder="Document"
          className={field}
        />
      </label>

      <label className="flex flex-col gap-1">
        <span className="text-text-secondary">Mode</span>
        <select
          value={pair.mode}
          onChange={(e) => setPair({ ...pair, mode: e.target.value as SyncMode })}
          className={field}
        >
          <option value="two-way">Two-way — changes flow in both directions</option>
          <option value="mirror-to-local">
            Mirror to computer — the device is the source of truth
          </option>
          <option value="mirror-to-remote">
            Mirror to device — this computer is the source of truth
          </option>
        </select>
      </label>

      <fieldset className="flex flex-col gap-1.5">
        <span className="text-text-secondary">Schedule</span>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={pair.on_connect}
            onChange={(e) => setPair({ ...pair, on_connect: e.target.checked })}
          />
          Run when the device connects
        </label>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={!!pair.interval_minutes}
            onChange={(e) =>
              setPair({ ...pair, interval_minutes: e.target.checked ? 30 : null })
            }
          />
          Run every
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
          minutes while connected
        </label>
      </fieldset>

      <label className="flex flex-col gap-1">
        <span className="text-text-secondary">
          Exclude patterns (one per line, e.g. <code>Note</code> or <code>Drafts/*</code>)
        </span>
        <textarea
          value={filtersText}
          onChange={(e) => setFiltersText(e.target.value)}
          rows={2}
          className={`${field} font-mono text-xs`}
        />
      </label>

      <label className="flex items-center justify-between gap-4">
        <span>
          Ask before deleting more than
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
          files
        </span>
      </label>

      <label className="flex items-center gap-2">
        <input
          type="checkbox"
          checked={pair.enabled}
          onChange={(e) => setPair({ ...pair, enabled: e.target.checked })}
        />
        Enabled
      </label>

      <div className="flex gap-2 border-t border-border pt-3">
        <button
          onClick={() => void save()}
          disabled={saving || !pair.local_root.trim()}
          className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground disabled:opacity-50"
        >
          {saving ? "Saving…" : "Save"}
        </button>
        <button
          onClick={onCancel}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

// ---- run history (FR-SYN-7) ------------------------------------------------------

function History({ pairId }: { pairId: string }) {
  const [records, setRecords] = useState<SyncRunRecord[] | null>(null);

  useEffect(() => {
    void ipc.syncHistory(pairId).then(setRecords, () => setRecords([]));
  }, [pairId]);

  if (records === null) {
    return (
      <p className="border-t border-border p-2.5 text-xs text-text-secondary">Loading…</p>
    );
  }
  if (records.length === 0) {
    return (
      <p className="border-t border-border p-2.5 text-xs text-text-secondary">
        No runs yet.
      </p>
    );
  }
  return (
    <ul className="max-h-48 overflow-auto border-t border-border text-xs">
      {records.map((r, i) => (
        <li key={i} className="border-b border-border p-2 last:border-b-0">
          <div className="flex justify-between gap-2">
            <span className="font-medium">{RESULT_LABEL[r.result] ?? r.result}</span>
            <span className="text-text-secondary">
              {formatTime(r.finished_at)} · {r.trigger}
              {r.device_serial ? ` · ${r.device_serial}` : ""}
            </span>
          </div>
          <div className="text-text-secondary">
            {r.done} done{r.failed > 0 ? `, ${r.failed} failed` : ""}
            {r.skipped > 0 ? `, ${r.skipped} skipped` : ""}
            {r.conflicts.length > 0
              ? ` · ${r.conflicts.length} conflict cop${r.conflicts.length === 1 ? "y" : "ies"}`
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
          <li className="p-1.5 text-xs text-text-secondary">None</li>
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
    <Dialog title="Sync preview" onClose={onClose} wide>
      {nothing ? (
        <p className="text-text-secondary">Everything is in sync — nothing to do.</p>
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
            {column("Uploads", [...groups.uploads, ...[]])}
            {column("Downloads", groups.downloads)}
            {column("Deletions", groups.deletions, (a) =>
              a.kind.startsWith("delete_local") ? "on this computer" : "on the device",
            )}
          </div>
          {groups.conflicts.length > 0 &&
            column(
              "Conflicts (newer side wins, loser kept as a copy)",
              groups.conflicts,
              (a) =>
                a.winner === "remote" ? "device version wins" : "computer version wins",
            )}
        </div>
      )}
      <div className="mt-4 flex gap-2 border-t border-border pt-3">
        {!nothing && (
          <button
            onClick={() => void apply()}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground"
          >
            Apply
          </button>
        )}
        <button
          onClick={onClose}
          className="border border-border px-3 py-1.5 hover:border-text"
        >
          {nothing ? "Close" : "Cancel"}
        </button>
      </div>
    </Dialog>
  );
}
