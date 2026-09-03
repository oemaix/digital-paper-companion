import { useApp } from "../lib/store";
import { CloseIcon } from "./icons";
import { useT } from "../lib/i18n";

/**
 * Toast stack, bottom-right (docs/05 §3.4). Monochrome: errors are marked
 * by a heavier border and an "!" prefix, not by color.
 */
export default function Toasts() {
  const t = useT();
  const toasts = useApp((s) => s.toasts);
  const dismiss = useApp((s) => s.dismissToast);
  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-10 end-3 z-50 flex w-80 flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          role="status"
          className={`flex items-start gap-2 border bg-bg px-3 py-2 text-[13px] shadow-lg ${
            toast.kind === "error" ? "border-2 border-text" : "border-border"
          }`}
        >
          {toast.kind === "error" && <span className="font-bold">!</span>}
          <span className="min-w-0 flex-1 break-words">{toast.text}</span>
          <button
            aria-label={t("common.dismiss")}
            onClick={() => dismiss(toast.id)}
            className="shrink-0 text-text-secondary hover:text-text"
          >
            <CloseIcon width={12} height={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
