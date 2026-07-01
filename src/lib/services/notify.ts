import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { settingsStore } from "$lib/stores/settings.svelte";

/**
 * Pošle OS notifikaci, pokud jsou notifikace v nastavení povolené
 * a uživatel udělil systémové oprávnění. Chyby se tiše spolknou —
 * notifikace nesmí shodit hlavní tok.
 */
export async function notify(title: string, body: string): Promise<void> {
  if (!settingsStore.notificationsEnabled) return;

  try {
    let granted = await isPermissionGranted();
    if (!granted) {
      const perm = await requestPermission();
      granted = perm === "granted";
    }
    if (granted) {
      sendNotification({ title, body });
    }
  } catch (e) {
    console.warn("Notifikace selhala:", e);
  }
}
