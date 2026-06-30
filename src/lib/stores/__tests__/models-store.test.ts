import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { modelsStore, formatBytes } from "$lib/stores/models.svelte";

const mockInvoke = vi.mocked(invoke);

describe("formatBytes", () => {
  it("formátuje běžné velikosti", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512.0 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(1024 * 1024)).toBe("1.0 MB");
    expect(formatBytes(4.1 * 1024 * 1024 * 1024)).toBe("4.1 GB");
  });

  it("nezáporné hraniční hodnoty", () => {
    expect(formatBytes(-5)).toBe("0 B");
  });
});

describe("modelsStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("load() načte modely a GPU", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_local_models")
        return [{ id: "m1", name: "Model 1", version: "v1", size_bytes: 1024, path: "/m1", checksum: "" }];
      if (cmd === "detect_gpu")
        return { name: "RTX 4090", vram_mb: 24576, backend: "cuda" };
      return null;
    });

    await modelsStore.load();

    expect(modelsStore.models).toHaveLength(1);
    expect(modelsStore.models[0].name).toBe("Model 1");
    expect(modelsStore.gpu?.backend).toBe("cuda");
  });

  it("deleteModel() odebere model ze seznamu", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_local_models")
        return [
          { id: "a", name: "A", version: "v", size_bytes: 1, path: "", checksum: "" },
          { id: "b", name: "B", version: "v", size_bytes: 1, path: "", checksum: "" },
        ];
      if (cmd === "detect_gpu") return null;
      return undefined;
    });
    await modelsStore.load();
    expect(modelsStore.models).toHaveLength(2);

    mockInvoke.mockResolvedValueOnce(undefined);
    await modelsStore.deleteModel("a");

    expect(mockInvoke).toHaveBeenCalledWith("delete_model", { modelId: "a" });
    expect(modelsStore.models.map((m) => m.id)).toEqual(["b"]);
  });
});
