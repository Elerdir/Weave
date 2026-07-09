import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { civitaiBrowserStore, type CatalogBrowseItem } from "$lib/stores/civitai-browser.svelte";

const mockInvoke = vi.mocked(invoke);

function sampleItem(overrides: Partial<CatalogBrowseItem> = {}): CatalogBrowseItem {
  return {
    name: "RealVis Ultra",
    creator: "sg161222",
    kind: "checkpoint",
    base_model: "SDXL 1.0",
    preview_image_url: "https://img/preview.jpg",
    downloads: 100,
    nsfw: false,
    file_name: "realvis_ultra.safetensors",
    download_url: "https://cdn/realvis_ultra.safetensors",
    size_bytes: 2048,
    trigger_words: [],
    ...overrides,
  };
}

describe("civitaiBrowserStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("search() volá backend s druhem modelu a ukládá výsledky", async () => {
    mockInvoke.mockResolvedValueOnce([sampleItem()]);

    civitaiBrowserStore.setKind("lora");
    civitaiBrowserStore.setQuery("  portrait  ");
    await civitaiBrowserStore.search();

    expect(mockInvoke).toHaveBeenCalledWith("browse_civitai", {
      query: "portrait",
      kind: "lora",
    });
    expect(civitaiBrowserStore.results).toHaveLength(1);
    expect(civitaiBrowserStore.searched).toBe(true);
  });

  it("search() s prázdným dotazem nevolá backend", async () => {
    civitaiBrowserStore.setQuery("   ");
    await civitaiBrowserStore.search();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("download() stáhne soubor a označí ho jako stažený", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);
    const item = sampleItem();

    await civitaiBrowserStore.download(item);

    expect(mockInvoke).toHaveBeenCalledWith("download_civitai_model", {
      kind: "checkpoint",
      fileName: "realvis_ultra.safetensors",
      downloadUrl: "https://cdn/realvis_ultra.safetensors",
    });
    expect(civitaiBrowserStore.isDownloaded("realvis_ultra.safetensors")).toBe(true);
    expect(civitaiBrowserStore.downloadingFile).toBeNull();
  });

  it("download() při chybě nastaví error a soubor neoznačí", async () => {
    mockInvoke.mockRejectedValueOnce("HTTP 401");
    const item = sampleItem({ file_name: "gated.safetensors" });

    await civitaiBrowserStore.download(item);

    expect(civitaiBrowserStore.error).toContain("401");
    expect(civitaiBrowserStore.isDownloaded("gated.safetensors")).toBe(false);
    expect(civitaiBrowserStore.downloadingFile).toBeNull();
  });
});
