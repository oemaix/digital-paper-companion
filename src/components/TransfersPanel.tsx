import { useApp } from "../lib/store";
import { ipc, type JobSnapshot } from "../lib/ipc";
import { CloseIcon, DownloadIcon, UploadIcon } from "./icons";
import { useT } from "../lib/i18n";

/**
 * Transfer queue panel (docs/05 §4.4): per-job progress, cancel, and a
 * clear-finished action. Docked to the right edge between toolbar and
 * status bar.
 */
export default function TransfersPanel({ onClose }: { onClose: () => void }) {
  const t = useT();
  const transfers = useApp((s) => s.transfers);
  const hasFinished = transfers.some(
    (t) => t.status !== "queued" && t.status !== "running",
  );

  return (
    <aside className="absolute bottom-7 end-0 top-10 z-40 flex w-80 flex-col border-s border-border bg-surface">
      <div className="flex items-center justify-between border-b border-border px-3 py-2">
        <h2 className="text-[13px] font-semibold">{t("transfers.title")}</h2>
        <div className="flex items-center gap-2">
          {hasFinished && (
            <button
              onClick={() => void ipc.transfersClearFinished()}
              className="text-xs text-text-secondary hover:text-text"
            >
              {t("transfers.clearFinished")}
            </button>
          )}
          <button
            aria-label={t("transfers.closeLabel")}
            onClick={onClose}
            className="text-text-secondary hover:text-text"
          >
            <CloseIcon />
          </button>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {transfers.length === 0 ? (
          <p className="px-3 py-4 text-xs text-text-secondary">{t("transfers.empty")}</p>
        ) : (
          <ul>
            {[...transfers].reverse().map((job) => (
              <TransferRow key={job.id} job={job} />
            ))}
          </ul>
        )}
      </div>
    </aside>
  );
}

function TransferRow({ job }: { job: JobSnapshot }) {
  const t = useT();
  const active = job.status === "queued" || job.status === "running";
  const statusLabel: Record<JobSnapshot["status"], string> = {
    queued: t("transfers.queued"),
    running:
      job.kind === "download" ? t("transfers.downloading") : t("transfers.uploading"),
    done: t("transfers.done"),
    failed: t("transfers.failed"),
    cancelled: t("transfers.cancelled"),
  };

  return (
    <li className="border-b border-border px-3 py-2">
      <div className="flex items-center gap-2">
        {job.kind === "download" ? (
          <DownloadIcon className="shrink-0 text-text-secondary" width={14} height={14} />
        ) : (
          <UploadIcon className="shrink-0 text-text-secondary" width={14} height={14} />
        )}
        <span className="min-w-0 flex-1 truncate" title={job.name}>
          {job.name}
        </span>
        {active && (
          <button
            onClick={() => void ipc.transferCancel(job.id)}
            className="shrink-0 text-xs text-text-secondary hover:text-text"
          >
            {t("common.cancel")}
          </button>
        )}
      </div>
      <div className="mt-1 flex items-center gap-2">
        {job.status === "running" ? (
          <div className="h-1 flex-1 border border-border">
            {job.progress != null ? (
              <div
                className="h-full bg-text"
                style={{ width: `${Math.round(job.progress * 100)}%` }}
              />
            ) : (
              <div className="h-full w-1/3 animate-pulse bg-text" />
            )}
          </div>
        ) : (
          <div className="flex-1" />
        )}
        <span className="shrink-0 text-xs tabular-nums text-text-secondary">
          {job.status === "running" && job.progress != null
            ? `${Math.round(job.progress * 100)} %`
            : statusLabel[job.status]}
        </span>
      </div>
      {job.error && job.status === "failed" && (
        <p className="mt-1 break-words text-xs text-text-secondary">! {job.error}</p>
      )}
    </li>
  );
}
