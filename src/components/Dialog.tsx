import { useEffect, useRef, type ReactNode } from "react";
import { CloseIcon } from "./icons";
import { useT } from "../lib/i18n";

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * Minimal modal dialog (docs/05 §3.4): sharp-cornered panel on a dimmed
 * backdrop; closes via the X button, backdrop click or Escape. Keyboard
 * focus is trapped inside and restored on close (NFR-UX-4).
 */
export default function Dialog({
  title,
  onClose,
  children,
  wide = false,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  const t = useT();
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // Initial focus into the dialog; restore focus on unmount (NFR-UX-4).
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const panel = panelRef.current;
    if (panel && !panel.contains(document.activeElement)) {
      const focusables = panel.querySelectorAll<HTMLElement>(FOCUSABLE);
      // Skip the close button when there is anything else to focus.
      (focusables[1] ?? focusables[0] ?? panel).focus();
    }
    return () => previous?.focus();
  }, []);

  // Focus trap: Tab cycles inside the panel.
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "Tab" || !panelRef.current) return;
    const focusables = [
      ...panelRef.current.querySelectorAll<HTMLElement>(FOCUSABLE),
    ].filter((el) => el.offsetParent !== null);
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      onMouseDown={onClose}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className={`${wide ? "w-[720px]" : "w-[440px]"} max-w-[90vw] border border-border bg-bg shadow-xl`}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <h2 className="text-sm font-semibold">{title}</h2>
          <button
            onClick={onClose}
            aria-label={t("common.close")}
            className="p-1 text-text-secondary hover:text-text"
          >
            <CloseIcon />
          </button>
        </div>
        <div className="max-h-[80vh] overflow-y-auto px-4 py-4">{children}</div>
      </div>
    </div>
  );
}
