// Lightweight i18n: Chinese source text is the message key and English
// translations live in `en.json`. Missing translations fall back to the
// Chinese text. Switching language reloads the page so that every string,
// including ones computed at module load, is rebuilt in the new language.
import en from "./en.json";

export type Locale = "zh-CN" | "en-US";

export const LOCALES: { value: Locale; label: string; short: string }[] = [
  { value: "zh-CN", label: "简体中文", short: "中" },
  { value: "en-US", label: "English", short: "EN" },
];

const storageKey = "gpt-images-admin-locale";
const english: Record<string, string> = en;

function detectLocale(): Locale {
  if (typeof window === "undefined") {
    return "zh-CN";
  }
  const stored = window.localStorage?.getItem(storageKey);
  if (stored === "zh-CN" || stored === "en-US") {
    return stored;
  }
  const preferred = window.navigator?.language ?? "zh-CN";
  return preferred.toLowerCase().startsWith("zh") ? "zh-CN" : "en-US";
}

export const locale: Locale = detectLocale();
export const isEnglish = locale === "en-US";

if (typeof document !== "undefined") {
  document.documentElement.lang = locale;
}

/**
 * Translates Chinese source text. Positional placeholders `{0}`, `{1}` are
 * filled from `args`, e.g. `t("剩余 {0} 天", days)`.
 */
export function t(text: string, ...args: unknown[]): string {
  const template = isEnglish ? (english[text] ?? text) : text;
  if (args.length === 0) {
    return template;
  }
  return template.replace(/\{(\d+)\}/g, (match, index: string) => {
    const value = args[Number(index)];
    return value === undefined || value === null ? "" : String(value);
  });
}

/** Translates text that comes from the server (menu titles, permission names). */
export function tData(text?: string | null): string {
  if (!text) {
    return "";
  }
  return isEnglish ? (english[text] ?? text) : text;
}

export function setLocale(next: Locale) {
  if (next === locale) {
    return;
  }
  window.localStorage.setItem(storageKey, next);
  window.location.reload();
}
