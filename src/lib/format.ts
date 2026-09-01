/** Formatting helpers (docs/05 §5.1: tabular numerals, locale dates). */
import { currentLocale } from "./i18n";

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return "—";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = "B";
  for (const u of units) {
    if (value < 1024) break;
    value /= 1024;
    unit = u;
  }
  return `${value >= 100 ? Math.round(value) : value.toFixed(1)} ${unit}`;
}

/** Device dates look like `2017-12-16T09:47:00Z`. */
export function formatDate(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleDateString(currentLocale(), {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/** Month heading for the notes view, e.g. "December 2017" (docs/05 §3.3). */
export function formatMonth(iso: string | null | undefined): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleDateString(currentLocale(), { year: "numeric", month: "long" });
}

/** Basename without the `.pdf` extension for display. */
export function displayName(name: string): string {
  return name.replace(/\.pdf$/i, "");
}
