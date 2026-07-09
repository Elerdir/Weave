import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { notify } from "$lib/services/notify";
import { settingsStore } from "$lib/stores/settings.svelte";

export interface LocalModel {
  id: string;
  name: string;
  version: string;
  size_bytes: number;
  path: string;
  checksum: string;
}

export interface RecommendedModel {
  id: string;
  name: string;
  description: string;
  size_bytes: number;
  download_url: string;
  recommended_gpu_layers: number;
}

export interface GpuInfo {
  name: string;
  vram_mb: number;
  backend: "cuda" | "metal" | "vulkan" | "cpu";
}

export interface DownloadState {
  modelId: string;
  downloaded: number;
  total: number;
  phase: "downloading" | "verifying";
  /** Aktuální rychlost stahování v bajtech/s (vyhlazený odhad). */
  speedBytesPerSec: number;
}

interface DownloadEvent {
  type: "started" | "progress" | "verifying" | "done" | "error";
  modelId: string;
  total?: number;
  downloaded?: number;
  message?: string;
}

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

/** Rychlost stahování jako čitelný text, např. „12.3 MB/s". */
export function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return "";
  return `${formatBytes(bytesPerSec)}/s`;
}

/** Zbývající čas stahování jako „m:ss" / „h:mm:ss"; prázdný, když nelze určit. */
export function formatEta(remainingBytes: number, bytesPerSec: number): string {
  if (bytesPerSec <= 0 || remainingBytes <= 0) return "";
  const secs = Math.round(remainingBytes / bytesPerSec);
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/**
 * Průběžný odhad rychlosti stahování. Progress eventy chodí po každém chunku
 * (velmi často), takže vzorkujeme nejvýš jednou za `SAMPLE_MS` a rychlost
 * vyhlazujeme klouzavým průměrem (EMA), aby číslo neposkakovalo.
 */
const SAMPLE_MS = 500;
function createSpeedMeter() {
  let lastTime = 0;
  let lastBytes = 0;
  let speed = 0;

  return {
    reset() {
      lastTime = performance.now();
      lastBytes = 0;
      speed = 0;
    },
    /** Zapíše nový počet stažených bajtů a vrátí aktuální odhad rychlosti. */
    update(downloaded: number): number {
      const now = performance.now();
      const dt = now - lastTime;
      if (dt >= SAMPLE_MS) {
        const inst = ((downloaded - lastBytes) / dt) * 1000;
        speed = speed === 0 ? inst : speed * 0.6 + inst * 0.4;
        lastTime = now;
        lastBytes = downloaded;
      }
      return speed;
    },
  };
}

function createModelsStore() {
  let models = $state<LocalModel[]>([]);
  let recommended = $state<RecommendedModel[]>([]);
  let gpu = $state<GpuInfo | null>(null);
  let download = $state<DownloadState | null>(null);
  let error = $state<string | null>(null);
  let modelsDir = $state<string>("");
  let movingModelsDir = $state(false);
  let downloadSegments = $state(16);

  return {
    get models() {
      return models;
    },
    get recommended() {
      return recommended;
    },
    get gpu() {
      return gpu;
    },
    get download() {
      return download;
    },
    get error() {
      return error;
    },
    get modelsDir() {
      return modelsDir;
    },
    get movingModelsDir() {
      return movingModelsDir;
    },
    get downloadSegments() {
      return downloadSegments;
    },

    isDownloaded(id: string) {
      return models.some((m) => m.id === id);
    },

    async load() {
      models = await invoke<LocalModel[]>("list_local_models");
      recommended = await invoke<RecommendedModel[]>("list_recommended_models");
      gpu = await invoke<GpuInfo | null>("detect_gpu");
      modelsDir = await invoke<string>("get_models_dir");
      downloadSegments = await invoke<number>("get_download_segments");
    },

    async setDownloadSegments(value: number) {
      const segments = Math.min(32, Math.max(1, Math.round(value)));
      downloadSegments = await invoke<number>("set_download_segments", { segments });
    },

    /** Přesune stahování (i existující modely) do jiné složky. */
    async setModelsDir(dir: string) {
      movingModelsDir = true;
      error = null;
      try {
        await invoke("set_models_dir", { dir });
        modelsDir = dir;
        models = await invoke<LocalModel[]>("list_local_models");
      } catch (err) {
        error = String(err);
      } finally {
        movingModelsDir = false;
      }
    },

    async deleteModel(modelId: string) {
      await invoke("delete_model", { modelId });
      models = models.filter((m) => m.id !== modelId);
    },

    async downloadModel(modelId: string, sourceUrl: string, sha256: string | null = null) {
      await runDownload(modelId, () => invoke("download_model", { modelId, sourceUrl, sha256 }));
    },

    /**
     * Stáhne doporučený model jedním tlačítkem — appka se po dokončení sama
     * přepne na vestavěnou GPU inferenci (backend/model_path/gpu_layers).
     */
    async downloadRecommended(modelId: string) {
      await runDownload(modelId, () => invoke("download_recommended_model", { modelId }));
      await settingsStore.load();
    },
  };

  async function runDownload(modelId: string, invokeDownload: () => Promise<unknown>) {
    error = null;
    download = { modelId, downloaded: 0, total: 0, phase: "downloading", speedBytesPerSec: 0 };
    const meter = createSpeedMeter();
    meter.reset();

    const unlisten = await listen<DownloadEvent>("model-download-progress", (e) => {
      const ev = e.payload;
      if (ev.type === "started") {
        meter.reset();
        download = {
          modelId,
          downloaded: 0,
          total: ev.total ?? 0,
          phase: "downloading",
          speedBytesPerSec: 0,
        };
      } else if (ev.type === "progress") {
        const downloaded = ev.downloaded ?? 0;
        download = {
          modelId,
          downloaded,
          total: ev.total ?? 0,
          phase: "downloading",
          speedBytesPerSec: meter.update(downloaded),
        };
      } else if (ev.type === "verifying") {
        if (download) download = { ...download, phase: "verifying", speedBytesPerSec: 0 };
      } else if (ev.type === "done") {
        download = null;
        unlisten();
        void modelsStore.load();
        void notify("Model stažen", `${modelId} je připraven k použití.`);
      } else if (ev.type === "error") {
        error = ev.message ?? "Stahování selhalo";
        download = null;
        unlisten();
      }
    });

    try {
      await invokeDownload();
    } catch (err) {
      error = String(err);
      download = null;
      unlisten();
    }
  }
}

export const modelsStore = createModelsStore();
