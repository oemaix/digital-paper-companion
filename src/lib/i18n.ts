/**
 * Minimal i18n (FR-APP-4, NFR-I18N-1): flat message catalogs per locale,
 * `{param}` substitution, and a zustand store so components re-render on
 * language change. No library dependency — two locales don't need one.
 *
 * - `t(key, params?)` works anywhere (also outside React, e.g. stores).
 * - `useT()` returns `t` and subscribes the component to locale changes.
 * - The language *setting* is `"system" | "en" | "de"` (persisted in the
 *   backend settings); the resolved locale is always a concrete one.
 */
import { create } from "zustand";
import { en, type MessageKey } from "./locales/en";
import { de } from "./locales/de";

export type { MessageKey };

const DICTS = { en, de } as const;
export type Locale = keyof typeof DICTS;

export const LOCALES: Locale[] = ["en", "de"];

/** Native display names for the language picker. */
export const LOCALE_LABEL: Record<Locale, string> = {
  en: "English",
  de: "Deutsch",
};

function resolveLocale(setting: string): Locale {
  if ((LOCALES as string[]).includes(setting)) return setting as Locale;
  const nav = (navigator.language ?? "en").toLowerCase();
  return nav.startsWith("de") ? "de" : "en";
}

interface I18nStore {
  /** The persisted setting: `"system"` or a locale code. */
  setting: string;
  /** The resolved, concrete locale. */
  locale: Locale;
  setLanguageSetting: (setting: string) => void;
}

export const useI18n = create<I18nStore>((set) => ({
  setting: "system",
  locale: resolveLocale("system"),
  setLanguageSetting: (setting) => {
    const locale = resolveLocale(setting);
    document.documentElement.lang = locale;
    set({ setting, locale });
  },
}));

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
