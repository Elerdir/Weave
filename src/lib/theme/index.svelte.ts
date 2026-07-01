export type Theme = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

const STORAGE_KEY = "weave.theme";

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
      localStorage.setItem(STORAGE_KEY, t);
    },
  };
}

export const themeStore = createThemeStore();
