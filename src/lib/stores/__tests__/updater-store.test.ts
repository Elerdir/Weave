import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { createUpdaterStore } from "$lib/stores/updater.svelte";

const mockCheck = vi.mocked(check);

function fakeUpdate(version: string, downloadAndInstall = vi.fn(() => Promise.resolve())): Update {
  return { version, body: `Novinky ${version}`, downloadAndInstall } as unknown as Update;
}

describe("updaterStore", () => {
  let store: ReturnType<typeof createUpdaterStore>;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    store = createUpdaterStore();
  });
  afterEach(() => {
    store.stopAutoCheck();
    vi.useRealTimers();
  });

  it("automatická kontrola proběhne až po prodlevě a pak v intervalu", async () => {
    mockCheck.mockResolvedValue(null);
    store.startAutoCheck(1_000, 60_000);

    expect(mockCheck).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(mockCheck).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(mockCheck).toHaveBeenCalledTimes(2);
  });

  it("opakované spuštění automatiky kontroly nezdvojí", async () => {
    mockCheck.mockResolvedValue(null);
    store.startAutoCheck(1_000, 60_000);
    store.startAutoCheck(1_000, 60_000);
    await vi.advanceTimersByTimeAsync(1_000);
    expect(mockCheck).toHaveBeenCalledTimes(1);
  });

  it("nalezená verze ukáže lištu s verzí a novinkami", async () => {
    mockCheck.mockResolvedValue(fakeUpdate("0.2.0"));
    await store.checkInBackground();

    expect(store.phase).toBe("available");
    expect(store.version).toBe("0.2.0");
    expect(store.notes).toBe("Novinky 0.2.0");
    expect(store.bannerVisible).toBe(true);
  });

  it("chyba při kontrole na pozadí se uživateli neukáže", async () => {
    mockCheck.mockRejectedValue(new Error("offline"));
    await store.checkInBackground();

    expect(store.phase).toBe("idle");
    expect(store.error).toBeNull();
    expect(store.bannerVisible).toBe(false);
  });

  it("odložená verze lištu skryje, novější ji ukáže znovu", async () => {
    mockCheck.mockResolvedValue(fakeUpdate("0.2.0"));
    await store.checkInBackground();
    store.dismiss();
    expect(store.bannerVisible).toBe(false);

    await store.checkInBackground();
    expect(store.bannerVisible).toBe(false);

    mockCheck.mockResolvedValue(fakeUpdate("0.3.0"));
    await store.checkInBackground();
    expect(store.bannerVisible).toBe(true);
  });

  it("stažení vede až k výzvě k restartu", async () => {
    mockCheck.mockResolvedValue(fakeUpdate("0.2.0"));
    await store.checkInBackground();
    await store.downloadAndInstall();

    expect(store.phase).toBe("readyToRestart");
    expect(store.bannerVisible).toBe(true);
  });

  it("selhaná instalace zůstane v liště i s možností opakovat", async () => {
    const failing = vi.fn(() => Promise.reject(new Error("podpis nesedí")));
    mockCheck.mockResolvedValue(fakeUpdate("0.2.0", failing));
    await store.checkInBackground();
    await store.downloadAndInstall();

    expect(store.phase).toBe("error");
    expect(store.error).toContain("podpis nesedí");
    expect(store.bannerVisible).toBe(true);
  });

  it("kontrola na pozadí nepřeruší rozjetou instalaci", async () => {
    mockCheck.mockResolvedValue(fakeUpdate("0.2.0"));
    await store.checkInBackground();
    await store.downloadAndInstall();
    mockCheck.mockClear();

    await store.checkInBackground();
    expect(mockCheck).not.toHaveBeenCalled();
    expect(store.phase).toBe("readyToRestart");
  });
});
