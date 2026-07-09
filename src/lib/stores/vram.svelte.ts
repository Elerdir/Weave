import { invoke } from "@tauri-apps/api/core";
import type { GpuInfo } from "$lib/stores/models.svelte";

/** Tvar odpovídá VramStatus v weave-shell (camelCase přes serde). */
export interface VramStatus {
  gpu: GpuInfo | null;
  embeddedLoaded: boolean;
  embeddedModel: string | null;
  comfyuiRunning: boolean;
  openvinoRunning: boolean;
}

const REFRESH_MS = 5000;

/**
 * VRAM indikátor v hlavičce chatu: využití paměti GPU (nvidia-smi) a kdo ji
 * právě drží (vestavěný LLM / ComfyUI / OpenVINO). Mimo Tauri (Playwright)
 * invoke selže → indikátor se prostě nezobrazí.
 */
function createVramStore() {
  let status = $state<VramStatus | null>(null);
  let timer: ReturnType<typeof setInterval> | null = null;

  async function refresh() {
    try {
      status = await invoke<VramStatus>("get_vram_status");
    } catch {
      status = null;
    }
  }

  return {
    get status() {
      return status;
    },

    /** Použitá VRAM v MB (total − free); null bez GPU nebo bez údaje o volné. */
    get usedMb(): number | null {
      const gpu = status?.gpu;
      if (!gpu || gpu.vram_mb <= 0 || gpu.free_vram_mb <= 0) return null;
      return Math.max(0, gpu.vram_mb - gpu.free_vram_mb);
    },

    /** Kdo právě drží VRAM — pro tooltip a tečku aktivity. */
    get holders(): string[] {
      if (!status) return [];
      const holders: string[] = [];
      if (status.embeddedLoaded) holders.push(status.embeddedModel ?? "LLM");
      if (status.comfyuiRunning) holders.push("ComfyUI");
      if (status.openvinoRunning) holders.push("OpenVINO");
      return holders;
    },

    refresh,

    /** Spustí periodické obnovování (idempotentní). */
    start() {
      if (timer) return;
      void refresh();
      timer = setInterval(() => void refresh(), REFRESH_MS);
    },

    stop() {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
    },
  };
}

export const vramStore = createVramStore();
