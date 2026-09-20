import { invoke } from "@tauri-apps/api/core";

export type Theme = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

/** Rychlá keš pro start bez probliknutí. Zdrojem pravdy je databáze. */
const STORAGE_KEY = "weave.theme";
/** Klíč v `app_config` (SQLite) — přežije i poškozená data WebView2. */
const SETTING_KEY = "app.theme";

function isTheme(value: unknown): value is Theme {
  return value === "light" || value === "dark" || value === "system";
}

function getSystemTheme(): ResolvedTheme {
  if (typeof window === "undefined") return "dark";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function applyTheme(resolved: ResolvedTheme) {
  document.documentElement.classList.remove("light", "dark");
  document.documentElement.classList.add(resolved);
}

function createThemeStore() {
  const stored = (localStorage.getItem(STORAGE_KEY) as Theme | null) ?? "system";
  let theme = $state<Theme>(stored);
  let resolvedTheme = $state<ResolvedTheme>(theme === "system" ? getSystemTheme() : theme);

  $effect.root(() => {
    // Vnořený $effect se znovu spustí při každé změně `theme` — přepnutí
    // motivu v nastavení se tak reálně promítne (jinak by se aplikoval jen
    // jednou při vzniku store).
    $effect(() => {
      const resolved = theme === "system" ? getSystemTheme() : theme;
      resolvedTheme = resolved;
      applyTheme(resolved);
    });

    // Sledování systémové preference jen v režimu "system".
    $effect(() => {
      if (theme !== "system") return;
      const mql = window.matchMedia("(prefers-color-scheme: dark)");
      const handler = (e: MediaQueryListEvent) => {
        resolvedTheme = e.matches ? "dark" : "light";
        applyTheme(resolvedTheme);
      };
      mql.addEventListener("change", handler);
      return () => mql.removeEventListener("change", handler);
    });
  });

  return {
    get theme() { return theme; },
    get resolvedTheme() { return resolvedTheme; },
    setTheme(t: Theme) {
      theme = t;
      // Databáze je zdroj pravdy, localStorage jen keš, aby start neproblikl
      // výchozím motivem, než dorazí odpověď z backendu.
      try {
        localStorage.setItem(STORAGE_KEY, t);
      } catch (err) {
        console.warn("Motiv nejde uložit do localStorage:", err);
      }
      invoke("set_app_setting", { key: SETTING_KEY, value: t }).catch((err) =>
        console.warn("Uložení motivu selhalo:", err)
      );
    },

    /**
     * Načte motiv z databáze. Volá se při startu — `localStorage` se totiž
     * drží v datové složce WebView2 a při jejím poškození se z něj nic
     * nepřečte (viz `needs_setup` v settings.rs).
     *
     * Když v databázi ještě nic není, přenese se tam hodnota z keše, aby
     * uživatel o svůj motiv nepřišel při přechodu na novou verzi.
     */
    async hydrate() {
      try {
        const stored = await invoke<string | null>("get_app_setting", { key: SETTING_KEY });
        if (isTheme(stored)) {
          theme = stored;
          return;
        }
        if (theme !== "system") {
          await invoke("set_app_setting", { key: SETTING_KEY, value: theme });
        }
      } catch (err) {
        console.warn("Načtení motivu z databáze selhalo:", err);
      }
    },
  };
}

export const themeStore = createThemeStore();
