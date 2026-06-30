import type { Messages } from "./locales/cs";
import cs from "./locales/cs";
import en from "./locales/en";

export type Locale = "cs" | "en";

const STORAGE_KEY = "weave.locale";
const BUNDLES: Record<Locale, Messages> = { cs, en };

function detectLocale(): Locale {
  const stored = localStorage.getItem(STORAGE_KEY) as Locale | null;
  if (stored && stored in BUNDLES) return stored;
  const nav = navigator.language?.slice(0, 2).toLowerCase();
  if (nav === "cs") return "cs";
  return "en";
}

function resolve(bundle: Messages, key: string): string {
  const parts = key.split(".");
  let node: unknown = bundle;
  for (const p of parts) {
    if (typeof node === "object" && node !== null && p in (node as object)) {
      node = (node as Record<string, unknown>)[p];
    } else {
      return key;
    }
  }
  return typeof node === "string" ? node : key;
}

function format(template: string, params?: Record<string, string | number>): string {
  if (!params) return template;
  return Object.entries(params).reduce(
    (acc, [k, v]) => acc.replace(new RegExp(`\\{${k}\\}`, "g"), String(v)),
    template
  );
}

function createI18nStore() {
  let locale = $state<Locale>(detectLocale());

  $effect.root(() => {
    document.documentElement.lang = locale;
  });

  return {
    get locale() { return locale; },
    setLocale(l: Locale) {
      locale = l;
      localStorage.setItem(STORAGE_KEY, l);
    },
    t(key: string, params?: Record<string, string | number>): string {
      return format(resolve(BUNDLES[locale], key), params);
    },
    /** Přistup k typovaným překladům přímo jako objekt */
    get m(): Messages {
      return BUNDLES[locale];
    },
  };
}

export const i18n = createI18nStore();
