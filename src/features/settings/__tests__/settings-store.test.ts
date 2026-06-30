import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { settingsStore } from "$lib/stores/settings.svelte";

const mockInvoke = vi.mocked(invoke);

describe("settingsStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("load() načte stav klíčů a ComfyUI URL", async () => {
    mockInvoke.mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "get_api_key_status") return args.service === "mistral";
      if (cmd === "get_masked_api_key") return "sk-a••••••••";
      if (cmd === "get_app_setting") return "http://localhost:9999";
      return null;
    });

    await settingsStore.load();

    expect(settingsStore.apiKeys.mistral.hasKey).toBe(true);
    expect(settingsStore.apiKeys.mistral.masked).toBe("sk-a••••••••");
    expect(settingsStore.apiKeys.civitai.hasKey).toBe(false);
    expect(settingsStore.comfyuiUrl).toBe("http://localhost:9999");
  });

  it("saveKey() uloží token a obnoví stav", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_api_key_status") return true;
      if (cmd === "get_masked_api_key") return "sk-x••••";
      return undefined;
    });

    await settingsStore.saveKey("mistral", "  sk-xyz  ");

    // token se ukládá oříznutý
    expect(mockInvoke).toHaveBeenCalledWith("store_api_key", {
      service: "mistral",
      token: "sk-xyz",
    });
    expect(settingsStore.apiKeys.mistral.hasKey).toBe(true);
  });

  it("testComfyui() nastaví connected při úspěchu", async () => {
    mockInvoke.mockResolvedValueOnce(true);
    await settingsStore.testComfyui();
    expect(settingsStore.comfyuiStatus).toBe("connected");
  });

  it("testComfyui() nastaví disconnected při chybě", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("offline"));
    await settingsStore.testComfyui();
    expect(settingsStore.comfyuiStatus).toBe("disconnected");
  });
});
