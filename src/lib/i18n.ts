/**
 * Minimal i18n (FR-APP-4, NFR-I18N-1): flat message catalogs per locale,
 * `{param}` substitution, and a zustand store so components re-render on
 * language change. No library dependency — flat catalogs keep the door
 * open for one if plural rules ever demand it.
 *
 * - `t(key, params?)` works anywhere (also outside React, e.g. stores).
 * - `useT()` returns `t` and subscribes the component to locale changes.
 * - The language *setting* is `"system"` or a locale code (persisted in
 *   the backend settings); the resolved locale is always a concrete one.
 * - Arabic and Hebrew flip the document to RTL (`dir` on `<html>`); the
 *   layout uses logical CSS properties so it mirrors automatically.
 */
import { create } from "zustand";
import { en, type MessageKey } from "./locales/en";
import { de } from "./locales/de";
import { es } from "./locales/es";
import { fr } from "./locales/fr";
import { it } from "./locales/it";
import { ja } from "./locales/ja";
import { zhHans } from "./locales/zhHans";
import { zhHant } from "./locales/zhHant";
import { ar } from "./locales/ar";
import { he } from "./locales/he";

export type { MessageKey };

const DICTS = {
  en,
  de,
  es,
  fr,
  it,
  ja,
  "zh-Hans": zhHans,
  "zh-Hant": zhHant,
  ar,
  he,
} as const;
export type Locale = keyof typeof DICTS;

export const LOCALES: Locale[] = [
  "en",
  "de",
  "es",
  "fr",
  "it",
  "ja",
  "zh-Hans",
  "zh-Hant",
  "ar",
  "he",
];

/** Native display names for the language picker. */
export const LOCALE_LABEL: Record<Locale, string> = {
  en: "English",
  de: "Deutsch",
  es: "Español",
  fr: "Français",
  it: "Italiano",
  ja: "日本語",
  "zh-Hans": "简体中文",
  "zh-Hant": "繁體中文",
  ar: "العربية",
  he: "עברית",
};

/** Right-to-left locales; drive `dir` on `<html>` (NFR-I18N-1 RTL pass). */
const RTL_LOCALES: ReadonlySet<Locale> = new Set(["ar", "he"]);

export function isRtl(locale: Locale): boolean {
  return RTL_LOCALES.has(locale);
}

/** Maps one BCP 47 tag to a supported locale, or null. */
function matchLocale(tag: string): Locale | null {
  const lower = tag.toLowerCase();
  if (lower.startsWith("zh")) {
    // Script subtag wins; fall back to the regions that customarily use
    // Traditional Chinese (TW, HK, MO), everything else is Simplified.
    return lower.includes("hant") ||
      lower.includes("-tw") ||
      lower.includes("-hk") ||
      lower.includes("-mo")
      ? "zh-Hant"
      : "zh-Hans";
  }
  const primary = lower.split("-")[0];
  return LOCALES.find((l) => l.split("-")[0] === primary) ?? null;
}

function resolveLocale(setting: string): Locale {
  if ((LOCALES as string[]).includes(setting)) return setting as Locale;
  const candidates = navigator.languages?.length
    ? navigator.languages
    : [navigator.language ?? "en"];
  for (const tag of candidates) {
    const match = matchLocale(tag);
    if (match) return match;
  }
  return "en";
}

/** Reflects the locale on `<html>` so text direction and fonts follow. */
function applyDocumentLocale(locale: Locale) {
  document.documentElement.lang = locale;
  document.documentElement.dir = isRtl(locale) ? "rtl" : "ltr";
}

interface I18nStore {
  /** The persisted setting: `"system"` or a locale code. */
  setting: string;
  /** The resolved, concrete locale. */
  locale: Locale;
  setLanguageSetting: (setting: string) => void;
}

export const useI18n = create<I18nStore>((set) => {
  const locale = resolveLocale("system");
  applyDocumentLocale(locale);
  return {
    setting: "system",
    locale,
    setLanguageSetting: (setting) => {
      const next = resolveLocale(setting);
      applyDocumentLocale(next);
      set({ setting, locale: next });
    },
  };
});

/** Translates a message key, substituting `{param}` placeholders. */
export function t(key: MessageKey, params?: Record<string, string | number>): string {
  const { locale } = useI18n.getState();
  let msg: string = DICTS[locale][key] ?? en[key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      msg = msg.replaceAll(`{${k}}`, String(v));
    }
  }
  return msg;
}

/** Hook variant: subscribes the component to locale changes. */
export function useT(): typeof t {
  useI18n((s) => s.locale);
  return t;
}

/** The resolved locale for `toLocale*String` formatting. */
export function currentLocale(): Locale {
  return useI18n.getState().locale;
}
