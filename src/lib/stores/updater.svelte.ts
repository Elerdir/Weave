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

/**
 * Kontrola a instalace aktualizací přes Tauri updater plugin (feed z UpdateHubu).
 * `check()` stáhne manifest, ověří Ed25519 podpis a vrátí `Update | null`.
 */
function createUpdaterStore() {
  let phase = $state<UpdaterPhase>("idle");
  let version = $state<string | null>(null);
  let notes = $state<string | null>(null);
  let error = $state<string | null>(null);
  let downloadedBytes = $state(0);
  let totalBytes = $state(0);

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
    checkForUpdate,
    downloadAndInstall,
    restart,
  };
}

export const updaterStore = createUpdaterStore();
