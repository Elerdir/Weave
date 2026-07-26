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
      if (cmd === "get_api_key_status") return args.service === "civitai";
      if (cmd === "get_masked_api_key") return "sk-a••••••••";
      if (cmd === "get_app_setting") return "http://localhost:9999";
      return null;
    });

    await settingsStore.load();

    expect(settingsStore.apiKeys.civitai.hasKey).toBe(true);
    expect(settingsStore.apiKeys.civitai.masked).toBe("sk-a••••••••");
    expect(settingsStore.apiKeys.huggingface.hasKey).toBe(false);
    expect(settingsStore.comfyuiUrl).toBe("http://localhost:9999");
  });

  it("saveKey() uloží token a obnoví stav", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "get_api_key_status") return true;
      if (cmd === "get_masked_api_key") return "sk-x••••";
      return undefined;
    });

    await settingsStore.saveKey("civitai", "  sk-xyz  ");

    // token se ukládá oříznutý
    expect(mockInvoke).toHaveBeenCalledWith("store_api_key", {
      service: "civitai",
      token: "sk-xyz",
    });
    expect(settingsStore.apiKeys.civitai.hasKey).toBe(true);
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

  it("load() načte LLM backend", async () => {
    mockInvoke.mockImplementation(async (cmd: string, args?: any) => {
      if (cmd === "get_api_key_status") return false;
      if (cmd === "get_app_setting") {
        if (args.key === "llm.backend") return "openvino_npu";
        return null;
      }
      return null;
    });

    await settingsStore.load();
    expect(settingsStore.llmBackend).toBe("openvino_npu");
  });

  it("setBackend() uloží volbu backendu", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await settingsStore.setBackend("openvino_npu");
    expect(settingsStore.llmBackend).toBe("openvino_npu");
    expect(mockInvoke).toHaveBeenCalledWith("set_app_setting", {
      key: "llm.backend",
      value: "openvino_npu",
    });
  });

  it("activateModel() uloží cestu a dopočítané gpu_layers z backendu", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "recommend_gpu_layers_for_path") return 0;
      return undefined;
    });

    await settingsStore.activateModel("C:\\models\\big-model.gguf");

    expect(mockInvoke).toHaveBeenCalledWith("set_app_setting", {
      key: "llm.model_path",
      value: "C:\\models\\big-model.gguf",
    });
    expect(mockInvoke).toHaveBeenCalledWith("recommend_gpu_layers_for_path", {
      path: "C:\\models\\big-model.gguf",
    });
    expect(settingsStore.gpuLayers).toBe("0");
  });

  it("activateModel() ponechá současné gpu_layers, když doporučení selže", async () => {
    const previousLayers = settingsStore.gpuLayers;
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "recommend_gpu_layers_for_path") throw new Error("no such file");
      return undefined;
    });

    await settingsStore.activateModel("C:\\models\\missing.gguf");

    expect(settingsStore.gpuLayers).toBe(previousLayers);
  });

  it("unloadEmbeddedModel() zavolá backend příkaz", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    await settingsStore.unloadEmbeddedModel();
    expect(mockInvoke).toHaveBeenCalledWith("unload_embedded_model");
  });
});
