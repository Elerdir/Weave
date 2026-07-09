import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { modelSearchStore, modelIdForFile } from "$lib/stores/model-search.svelte";

const mockInvoke = vi.mocked(invoke);

describe("modelIdForFile", () => {
  it("odvodí id modelu ze stemu GGUF souboru", () => {
    expect(modelIdForFile("Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf")).toBe(
      "Meta-Llama-3.1-8B-Instruct-Q4_K_M"
    );
    // Cesta v podsložce → jen jméno souboru; nebezpečné znaky → pomlčka
    expect(modelIdForFile("sub/dir/model q4.GGUF")).toBe("model-q4");
  });
});

describe("modelSearchStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("search() volá backend a uloží výsledky", async () => {
    mockInvoke.mockResolvedValueOnce([
      {
        repo_id: "bartowski/Llama-GGUF",
        author: "bartowski",
        name: "Llama-GGUF",
        downloads: 100,
        likes: 5,
        gated: false,
      },
    ]);

    modelSearchStore.setQuery("  llama  ");
    await modelSearchStore.search();

    expect(mockInvoke).toHaveBeenCalledWith("search_model_catalog", { query: "llama" });
    expect(modelSearchStore.results).toHaveLength(1);
    expect(modelSearchStore.searched).toBe(true);
    expect(modelSearchStore.error).toBeNull();
  });

  it("search() s prázdným dotazem nevolá backend", async () => {
    modelSearchStore.setQuery("   ");
    await modelSearchStore.search();
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("toggleRepo() dotáhne kvantizace jednou a kešuje je", async () => {
    mockInvoke.mockResolvedValueOnce([
      {
        file_name: "m.Q4_K_M.gguf",
        size_bytes: 4000,
        quant: "Q4_K_M",
        download_url: "https://hf/x",
      },
    ]);

    await modelSearchStore.toggleRepo("org/repo");
    expect(modelSearchStore.expandedRepo).toBe("org/repo");
    expect(modelSearchStore.filesFor("org/repo")).toHaveLength(1);
    expect(mockInvoke).toHaveBeenCalledWith("list_catalog_gguf_files", { repoId: "org/repo" });

    // Sbalit a znovu rozbalit → žádný další fetch (cache)
    await modelSearchStore.toggleRepo("org/repo");
    expect(modelSearchStore.expandedRepo).toBeNull();
    await modelSearchStore.toggleRepo("org/repo");
    expect(mockInvoke).toHaveBeenCalledTimes(1);
  });

  it("toggleRepo() při chybě sbalí repo a nastaví error", async () => {
    mockInvoke.mockRejectedValueOnce("401 gated");

    await modelSearchStore.toggleRepo("meta/llama");

    expect(modelSearchStore.expandedRepo).toBeNull();
    expect(modelSearchStore.error).toContain("401");
  });
});
