import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  modelsStore,
  formatBytes,
  formatSpeed,
  formatEta,
} from "$lib/stores/models.svelte";

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

describe("formatSpeed", () => {
  it("přidá /s k velikosti", () => {
    expect(formatSpeed(12.3 * 1024 * 1024)).toBe("12.3 MB/s");
    expect(formatSpeed(1024)).toBe("1.0 KB/s");
  });

  it("nula nebo záporné = prázdné", () => {
    expect(formatSpeed(0)).toBe("");
    expect(formatSpeed(-1)).toBe("");
  });
});

describe("formatEta", () => {
  it("formátuje zbývající čas", () => {
    // 10 MB zbývá při 1 MB/s = 10 s → 0:10
    expect(formatEta(10 * 1024 * 1024, 1024 * 1024)).toBe("0:10");
    // 150 s → 2:30
    expect(formatEta(150 * 1024 * 1024, 1024 * 1024)).toBe("2:30");
    // přes hodinu → h:mm:ss
    expect(formatEta(3661 * 1024 * 1024, 1024 * 1024)).toBe("1:01:01");
  });

  it("bez rychlosti nebo zbytku = prázdné", () => {
    expect(formatEta(1000, 0)).toBe("");
    expect(formatEta(0, 1000)).toBe("");
  });
});

describe("modelsStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("load() načte modely, doporučené modely a GPU", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_local_models")
        return [{ id: "m1", name: "Model 1", version: "v1", size_bytes: 1024, path: "/m1", checksum: "" }];
      if (cmd === "list_recommended_models")
        return [{ id: "qwen2.5-1.5b-instruct", name: "Qwen2.5 1.5B", description: "d", size_bytes: 1, download_url: "u", recommended_gpu_layers: 999 }];
      if (cmd === "detect_gpu")
        return { name: "RTX 4090", vram_mb: 24576, backend: "cuda" };
      return null;
    });

    await modelsStore.load();

    expect(modelsStore.models).toHaveLength(1);
    expect(modelsStore.models[0].name).toBe("Model 1");
    expect(modelsStore.recommended).toHaveLength(1);
    expect(modelsStore.gpu?.backend).toBe("cuda");
  });

  it("isDownloaded() reflektuje stažené modely", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_local_models")
        return [{ id: "m1", name: "M1", version: "v", size_bytes: 1, path: "", checksum: "" }];
      if (cmd === "list_recommended_models") return [];
      if (cmd === "detect_gpu") return null;
      return undefined;
    });
    await modelsStore.load();
    expect(modelsStore.isDownloaded("m1")).toBe(true);
    expect(modelsStore.isDownloaded("neexistuje")).toBe(false);
  });

  it("deleteModel() odebere model ze seznamu", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "list_local_models")
        return [
          { id: "a", name: "A", version: "v", size_bytes: 1, path: "", checksum: "" },
          { id: "b", name: "B", version: "v", size_bytes: 1, path: "", checksum: "" },
        ];
      if (cmd === "list_recommended_models") return [];
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

  it("downloadRecommended() zavolá download_recommended_model a reloaduje settings", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "download_recommended_model") return undefined;
      if (cmd === "list_local_models") return [];
      if (cmd === "list_recommended_models") return [];
      if (cmd === "detect_gpu") return null;
      if (cmd === "get_api_key_status") return false;
      if (cmd === "get_app_setting") return null;
      return undefined;
    });

    await modelsStore.downloadRecommended("qwen2.5-1.5b-instruct");

    expect(mockInvoke).toHaveBeenCalledWith("download_recommended_model", {
      modelId: "qwen2.5-1.5b-instruct",
    });
  });

  // Tlacitka Stahnout jsou disabled pres `!!modelsStore.download`. Kdyz stav
  // zustane viset, appka uz na zadne kliknuti nereaguje — proto se musi
  // uvolnit i kdyz backend nevrati zadny terminalni event nebo spadne.
  it("po dokonceni bez 'done' eventu neuvazne stav stahovani", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "download_recommended_model") return undefined;
      if (cmd === "list_local_models") return [];
      if (cmd === "list_recommended_models") return [];
      if (cmd === "detect_gpu") return null;
      return undefined;
    });

    await modelsStore.downloadRecommended("qwen2.5-1.5b-instruct");

    expect(modelsStore.download).toBeNull();
  });

  it("po chybe backendu neuvazne stav stahovani a ohlasi chybu", async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === "download_model") throw new Error("stahovani selhalo");
      if (cmd === "list_local_models") return [];
      if (cmd === "list_recommended_models") return [];
      if (cmd === "detect_gpu") return null;
      return undefined;
    });

    await modelsStore.downloadModel("nejaky-model", "https://example.invalid/m.gguf");

    expect(modelsStore.download).toBeNull();
    expect(modelsStore.error).toContain("stahovani selhalo");
  });

  // Realny pad: okno "settings" chybelo v capabilities, takze listen() na core
  // eventy vyhodil vyjimku jeste pred invoke. Stav zustal viset a vsechna
  // tlacitka Stahnout byla navzdy disabled — appka "nereagovala na kliknuti".
  it("kdyz listen() selze (chybi capability), stav se uvolni a chyba se ohlasi", async () => {
    const { listen } = await import("@tauri-apps/api/event");
    vi.mocked(listen).mockRejectedValueOnce(
      new Error('event.listen not allowed on window "settings"'),
    );
    mockInvoke.mockImplementation(async () => undefined);

    await modelsStore.downloadRecommended("qwen2.5-1.5b-instruct");

    expect(modelsStore.download).toBeNull();
    expect(modelsStore.error).toContain("not allowed on window");
  });

  it("setError() zverejni chybu z UI (napr. pad dialogu vyberu slozky)", () => {
    modelsStore.setError("Nepodařilo se otevřít výběr složky");
    expect(modelsStore.error).toContain("výběr složky");
    modelsStore.setError(null);
    expect(modelsStore.error).toBeNull();
  });
});
