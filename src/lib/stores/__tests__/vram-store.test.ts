import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { vramStore, type VramStatus } from "$lib/stores/vram.svelte";

const mockInvoke = vi.mocked(invoke);

function status(overrides: Partial<VramStatus> = {}): VramStatus {
  return {
    gpu: {
      name: "RTX 3090",
      vram_mb: 24576,
      free_vram_mb: 10576,
      backend: "cuda",
    },
    embeddedLoaded: false,
    embeddedModel: null,
    comfyuiRunning: false,
    openvinoRunning: false,
    ...overrides,
  };
}

describe("vramStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });
  afterEach(() => {
    vramStore.stop();
  });

  it("refresh() načte stav a spočítá použitou VRAM", async () => {
    mockInvoke.mockResolvedValueOnce(status());

    await vramStore.refresh();

    expect(mockInvoke).toHaveBeenCalledWith("get_vram_status");
    expect(vramStore.usedMb).toBe(24576 - 10576);
  });

  it("usedMb je null bez GPU nebo bez údaje o volné paměti", async () => {
    mockInvoke.mockResolvedValueOnce(status({ gpu: null }));
    await vramStore.refresh();
    expect(vramStore.usedMb).toBeNull();

    // free_vram_mb = 0 znamená „nezjištěno" (non-NVIDIA) → žádné číslo
    mockInvoke.mockResolvedValueOnce(
      status({ gpu: { name: "iGPU", vram_mb: 8192, free_vram_mb: 0, backend: "vulkan" } })
    );
    await vramStore.refresh();
    expect(vramStore.usedMb).toBeNull();
  });

  it("holders vyjmenuje, kdo paměť drží (LLM podle názvu souboru)", async () => {
    mockInvoke.mockResolvedValueOnce(
      status({
        embeddedLoaded: true,
        embeddedModel: "gemma-3-27b.gguf",
        comfyuiRunning: true,
      })
    );

    await vramStore.refresh();

    expect(vramStore.holders).toEqual(["gemma-3-27b.gguf", "ComfyUI"]);
  });

  it("mimo Tauri (invoke selže) je stav null a nic nespadne", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("no tauri"));
    await vramStore.refresh();
    expect(vramStore.status).toBeNull();
    expect(vramStore.holders).toEqual([]);
  });
});
