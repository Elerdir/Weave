import { invoke } from "@tauri-apps/api/core";

import type { Messages } from "./locales/cs";
import cs from "./locales/cs";
import en from "./locales/en";

export type Locale = "cs" | "en";

/** Rychlá keš pro start bez probliknutí. Zdrojem pravdy je databáze. */
const STORAGE_KEY = "weave.locale";
/** Klíč v `app_config` (SQLite) — přežije i poškozená data WebView2. */
const SETTING_KEY = "app.locale";
const BUNDLES: Record<Locale, Messages> = { cs, en };

function isLocale(value: unknown): value is Locale {
  return typeof value === "string" && value in BUNDLES;
}

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
    // Vnořený $effect: kořen se spustí jen jednou, takže bez něj by se `lang`
    // po přepnutí jazyka neaktualizoval.
    $effect(() => {
      document.documentElement.lang = locale;
    });
  });

  return {
    get locale() { return locale; },
    setLocale(l: Locale) {
      locale = l;
      // Databáze je zdroj pravdy, localStorage jen keš pro rychlý start.
      try {
        localStorage.setItem(STORAGE_KEY, l);
      } catch (err) {
        console.warn("Jazyk nejde uložit do localStorage:", err);
      }
      invoke("set_app_setting", { key: SETTING_KEY, value: l }).catch((err) =>
        console.warn("Uložení jazyka selhalo:", err)
      );
    },

    /**
     * Načte jazyk z databáze. Volá se při startu — `localStorage` se drží
     * v datové složce WebView2 a při jejím poškození se z něj nic nepřečte.
     *
     * Když v databázi nic není, přenese se tam dosavadní volba (z keše nebo
     * z jazyka systému), ať uživatel o nastavení nepřijde.
     */
    async hydrate() {
      try {
        const stored = await invoke<string | null>("get_app_setting", { key: SETTING_KEY });
        if (isLocale(stored)) {
          locale = stored;
          return;
        }
        await invoke("set_app_setting", { key: SETTING_KEY, value: locale });
      } catch (err) {
        console.warn("Načtení jazyka z databáze selhalo:", err);
      }
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
