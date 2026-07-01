import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  isPermissionGranted,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { notify } from "$lib/services/notify";
import { settingsStore } from "$lib/stores/settings.svelte";

vi.mock("@tauri-apps/plugin-notification", () => ({
  isPermissionGranted: vi.fn(async () => true),
  requestPermission: vi.fn(async () => "granted"),
  sendNotification: vi.fn(),
}));

const mockSend = vi.mocked(sendNotification);
const mockGranted = vi.mocked(isPermissionGranted);

describe("notify", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGranted.mockResolvedValue(true);
  });

  it("pošle notifikaci když jsou povolené a je oprávnění", async () => {
    await settingsStore.setNotifications(true);
    await notify("Titulek", "Tělo");
    expect(mockSend).toHaveBeenCalledWith({ title: "Titulek", body: "Tělo" });
  });

  it("nepošle nic když jsou notifikace vypnuté", async () => {
    await settingsStore.setNotifications(false);
    await notify("Titulek", "Tělo");
    expect(mockSend).not.toHaveBeenCalled();
    // vrátíme zpět pro ostatní testy
    await settingsStore.setNotifications(true);
  });

  it("nepošle nic bez systémového oprávnění", async () => {
    await settingsStore.setNotifications(true);
    mockGranted.mockResolvedValue(false);
    const { requestPermission } = await import("@tauri-apps/plugin-notification");
    vi.mocked(requestPermission).mockResolvedValue("denied");
    await notify("Titulek", "Tělo");
    expect(mockSend).not.toHaveBeenCalled();
  });
});
