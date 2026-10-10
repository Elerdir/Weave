import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

/** Fáze toku aktualizace pro UI. */
export type UpdaterPhase =
  | "idle"
  | "checking"
  | "available"
  | "upToDate"
  | "downloading"
  | "readyToRestart"
  | "error";

/** První kontrola na pozadí až chvíli po startu — ať nesoupeří s načítáním modelu. */
export const AUTO_CHECK_DELAY_MS = 10_000;
/** Aplikace bývá otevřená celé dny, proto se kontroluje i průběžně. */
export const AUTO_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

/**
 * Kontrola a instalace aktualizací přes Tauri updater plugin (feed z UpdateHubu).
 * `check()` stáhne manifest, ověří Ed25519 podpis a vrátí `Update | null`.
 *
 * Dva vstupy: ruční kontrola v Nastavení (`checkForUpdate`, ukazuje i chyby)
 * a automatická na pozadí (`startAutoCheck`), která chyby jen zaloguje —
 * výpadek UpdateHubu nebo offline stroj nesmí uživatele obtěžovat hláškou.
 */
export function createUpdaterStore() {
  let phase = $state<UpdaterPhase>("idle");
  let version = $state<string | null>(null);
  let notes = $state<string | null>(null);
  let error = $state<string | null>(null);
  let downloadedBytes = $state(0);
  let totalBytes = $state(0);
  // Verze, kterou uživatel v liště odložil „na později". Nová verze lištu
  // ukáže znovu.
  let dismissedVersion = $state<string | null>(null);
  let timers: { delay?: ReturnType<typeof setTimeout>; interval?: ReturnType<typeof setInterval> } = {};

  // Nesdílíme přes $state — je to jen handle pro navazující instalaci.
  let pending: Update | null = null;

  const percent = $derived(
    totalBytes > 0 ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100)) : 0
  );

  async function checkForUpdate(): Promise<void> {
    phase = "checking";
    error = null;
    try {
      const update = await check();
      if (update) {
        pending = update;
        version = update.version;
        notes = update.body ?? null;
        phase = "available";
      } else {
        pending = null;
        phase = "upToDate";
      }
    } catch (e) {
      error = String(e);
      phase = "error";
    }
  }

  /**
   * Tichá kontrola pro automatiku. Nemění fázi na `checking` (lišta by
   * problikla) a do rozjeté instalace nesahá.
   */
  async function checkInBackground(): Promise<void> {
    if (phase === "checking" || phase === "downloading" || phase === "readyToRestart") return;
    try {
      const update = await check();
      if (update) {
        pending = update;
        version = update.version;
        notes = update.body ?? null;
        error = null;
        phase = "available";
      }
    } catch (e) {
      console.warn("Automatická kontrola aktualizací selhala:", e);
    }
  }

  /** Spustí kontrolu po startu a pak v intervalu. Opakované volání nic nezdvojí. */
  function startAutoCheck(
    delayMs = AUTO_CHECK_DELAY_MS,
    intervalMs = AUTO_CHECK_INTERVAL_MS
  ): void {
    if (timers.delay || timers.interval) return;
    timers.delay = setTimeout(() => void checkInBackground(), delayMs);
    timers.interval = setInterval(() => void checkInBackground(), intervalMs);
  }

  function stopAutoCheck(): void {
    clearTimeout(timers.delay);
    clearInterval(timers.interval);
    timers = {};
  }

  /** Odloží nabídku aktuální verze (lišta zmizí do vydání další). */
  function dismiss(): void {
    dismissedVersion = version;
  }

  async function downloadAndInstall(): Promise<void> {
    if (!pending) return;
    phase = "downloading";
    downloadedBytes = 0;
    totalBytes = 0;
    try {
      await pending.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            totalBytes = event.data.contentLength ?? 0;
            break;
          case "Progress":
            downloadedBytes += event.data.chunkLength;
            break;
          case "Finished":
            downloadedBytes = totalBytes;
            break;
        }
      });
      phase = "readyToRestart";
    } catch (e) {
      error = String(e);
      phase = "error";
    }
  }

  /** Restartuje aplikaci, aby se nainstalovaná aktualizace projevila. */
  async function restart(): Promise<void> {
    await relaunch();
  }

  return {
    get phase() {
      return phase;
    },
    get version() {
      return version;
    },
    get notes() {
      return notes;
    },
    get error() {
      return error;
    },
    get percent() {
      return percent;
    },
    /**
     * Má se ukázat lišta v hlavním okně? Ano, dokud je co nabídnout nebo
     * dokončit; chyba se ukáže jen tehdy, když spadla rozjetá instalace.
     */
    get bannerVisible() {
      if (phase === "downloading" || phase === "readyToRestart") return true;
      if (phase === "error") return pending !== null;
      return phase === "available" && dismissedVersion !== version;
    },
    checkForUpdate,
    checkInBackground,
    startAutoCheck,
    stopAutoCheck,
    dismiss,
    downloadAndInstall,
    restart,
  };
}

export const updaterStore = createUpdaterStore();
