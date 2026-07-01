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

function createModelsStore() {
  let models = $state<LocalModel[]>([]);
  let recommended = $state<RecommendedModel[]>([]);
  let gpu = $state<GpuInfo | null>(null);
  let download = $state<DownloadState | null>(null);
  let error = $state<string | null>(null);

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

    isDownloaded(id: string) {
      return models.some((m) => m.id === id);
    },

    async load() {
      models = await invoke<LocalModel[]>("list_local_models");
      recommended = await invoke<RecommendedModel[]>("list_recommended_models");
      gpu = await invoke<GpuInfo | null>("detect_gpu");
    },

    async deleteModel(modelId: string) {
      await invoke("delete_model", { modelId });
      models = models.filter((m) => m.id !== modelId);
    },

    async downloadModel(modelId: string, sourceUrl: string) {
      await runDownload(modelId, () => invoke("download_model", { modelId, sourceUrl }));
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
    download = { modelId, downloaded: 0, total: 0, phase: "downloading" };

    const unlisten = await listen<DownloadEvent>("model-download-progress", (e) => {
      const ev = e.payload;
      if (ev.type === "started") {
        download = { modelId, downloaded: 0, total: ev.total ?? 0, phase: "downloading" };
      } else if (ev.type === "progress") {
        download = {
          modelId,
          downloaded: ev.downloaded ?? 0,
          total: ev.total ?? 0,
          phase: "downloading",
        };
      } else if (ev.type === "verifying") {
        if (download) download = { ...download, phase: "verifying" };
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
