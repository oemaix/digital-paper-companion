import Dialog from "./Dialog";
import { useApp } from "../lib/store";
import { ipc, errorMessage, type SyncConfirmationRequest } from "../lib/ipc";

/**
 * Mass-deletion gate (docs/06 §5.6; FR-SYN-5): a paused sync run lists the
 * planned deletions; the user applies them, skips deletions only, or
 * cancels the run.
 */
export default function SyncConfirmDialog({
  request,
}: {
  request: SyncConfirmationRequest;
}) {
  const toast = useApp((s) => s.toast);

  const decide = async (decision: "apply" | "skip-deletions" | "cancel") => {
    try {
      await ipc.syncConfirm(request.pair_id, decision);
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const list = (title: string, items: string[]) =>
    items.length > 0 && (
      <div className="min-w-0 flex-1">
        <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-text-secondary">
          {title} ({items.length})
        </h3>
        <ul className="max-h-48 overflow-auto border border-border text-xs">
          {items.map((p) => (
            <li
              key={p}
              className="truncate border-b border-border p-1.5 last:border-b-0"
              title={p}
            >
              {p}
            </li>
          ))}
        </ul>
      </div>
    );

  const total = request.local_deletions.length + request.remote_deletions.length;

  return (
    <Dialog title="Confirm deletions" onClose={() => void decide("cancel")} wide>
      <div className="flex flex-col gap-3 text-[13px]">
        <p>
          Syncing “{request.pair_name}” would delete <strong>{total}</strong> items — more
          than the configured threshold of {request.threshold}. Deletions on the device
          can't be undone.
        </p>
        <div className="flex gap-3">
          {list("On this computer", request.local_deletions)}
          {list("On the device", request.remote_deletions)}
        </div>
        <div className="flex gap-2 border-t border-border pt-3">
          <button
            onClick={() => void decide("apply")}
            className="border border-accent bg-accent px-3 py-1.5 text-accent-foreground"
          >
            Delete and continue
          </button>
          <button
            onClick={() => void decide("skip-deletions")}
            className="border border-border px-3 py-1.5 hover:border-text"
          >
            Sync without deleting
          </button>
          <button
            onClick={() => void decide("cancel")}
            className="ml-auto border border-border px-3 py-1.5 text-text-secondary hover:border-text hover:text-text"
          >
            Cancel run
          </button>
        </div>
      </div>
    </Dialog>
  );
}
