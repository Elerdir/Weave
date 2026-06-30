import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface LocalModel {
  id: string;
  name: string;
  version: string;
  size_bytes: number;
  path: string;
  checksum: string;
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
  let gpu = $state<GpuInfo | null>(null);
  let download = $state<DownloadState | null>(null);
  let error = $state<string | null>(null);

  return {
    get models() {
      return models;
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

    async load() {
      models = await invoke<LocalModel[]>("list_local_models");
      gpu = await invoke<GpuInfo | null>("detect_gpu");
    },

    async deleteModel(modelId: string) {
      await invoke("delete_model", { modelId });
      models = models.filter((m) => m.id !== modelId);
    },

    async downloadModel(modelId: string, sourceUrl: string) {
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
          void this.load();
        } else if (ev.type === "error") {
          error = ev.message ?? "Stahování selhalo";
          download = null;
          unlisten();
        }
      });

      try {
        await invoke("download_model", { modelId, sourceUrl });
      } catch (err) {
        error = String(err);
        download = null;
        unlisten();
      }
    },
  };
}

export const modelsStore = createModelsStore();
