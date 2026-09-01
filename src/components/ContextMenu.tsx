import { useEffect, useRef } from "react";

export interface MenuItem {
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  separator?: boolean;
}

/**
 * Sharp-cornered context menu (docs/05 §4.3). Closes on outside click,
 * Escape, or item activation. Fully keyboard-operable: arrow keys move
 * between enabled items, Home/End jump, Enter/Space activate (NFR-UX-4).
 */
export default function ContextMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
    };
  }, [onClose]);

  const focusables = () => [
    ...(ref.current?.querySelectorAll<HTMLButtonElement>(
      "button[role='menuitem']:not(:disabled)",
    ) ?? []),
  ];

  // Focus the first enabled item so arrow keys work immediately.
  useEffect(() => {
    focusables()[0]?.focus();
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    const els = focusables();
    if (els.length === 0) return;
    const idx = els.indexOf(document.activeElement as HTMLButtonElement);
    const focus = (i: number) => {
      e.preventDefault();
      els[(i + els.length) % els.length].focus();
    };
    if (e.key === "ArrowDown") focus(idx + 1);
    else if (e.key === "ArrowUp") focus(idx - 1);
    else if (e.key === "Home") focus(0);
    else if (e.key === "End") focus(els.length - 1);
    else if (e.key === "Tab") onClose();
  };

  // Keep the menu inside the viewport.
  const style: React.CSSProperties = {
    left: Math.min(x, window.innerWidth - 180),
    top: Math.min(y, window.innerHeight - items.length * 30 - 16),
  };

  return (
    <div
      ref={ref}
      role="menu"
      onKeyDown={onKeyDown}
      className="fixed z-50 min-w-44 border border-border bg-bg py-1 shadow-lg"
      style={style}
    >
      {items.map((item, i) =>
        item.separator ? (
          <div key={i} role="separator" className="my-1 h-px bg-border" />
        ) : (
          <button
            key={i}
            role="menuitem"
            disabled={item.disabled}
            onClick={() => {
              onClose();
              item.onClick?.();
            }}
            className="block w-full px-3 py-1.5 text-left text-[13px] hover:bg-accent hover:text-accent-foreground focus:bg-accent focus:text-accent-foreground focus:outline-none disabled:opacity-40 disabled:hover:bg-transparent disabled:hover:text-text"
          >
            {item.label}
          </button>
        ),
      )}
    </div>
  );
}
