import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Tvar odpovídá ImageModelKind/CatalogBrowseItem v weave-application. */
export type ImageModelKind = "checkpoint" | "lora";

export interface CatalogBrowseItem {
  name: string;
  creator: string;
  kind: ImageModelKind;
  base_model: string;
  preview_image_url: string | null;
  downloads: number;
  nsfw: boolean;
  file_name: string;
  download_url: string;
  size_bytes: number;
  trigger_words: string[];
}

interface DownloadEvent {
  type: "step" | "output" | "done" | "error";
  file: string;
  name?: string;
  line?: string;
  message?: string;
}

/** Prohlížeč CivitAI (Nastavení → ComfyUI): checkpointy a LoRA s náhledy. */
function createCivitaiBrowserStore() {
  let query = $state("");
  let kind = $state<ImageModelKind>("checkpoint");
  let results = $state<CatalogBrowseItem[]>([]);
  let searched = $state(false);
  let searching = $state(false);
  let error = $state<string | null>(null);
  /** Soubor právě ve stahování (max jedno najednou) + poslední řádek průběhu. */
  let downloadingFile = $state<string | null>(null);
  let progressLine = $state("");
  let downloadedFiles = $state<Set<string>>(new Set());

  return {
    get query() {
      return query;
    },
    get kind() {
      return kind;
    },
    get results() {
      return results;
    },
    get searched() {
      return searched;
    },
    get searching() {
      return searching;
    },
    get error() {
      return error;
    },
    get downloadingFile() {
      return downloadingFile;
    },
    get progressLine() {
      return progressLine;
    },

    setQuery(value: string) {
      query = value;
    },

    setKind(value: ImageModelKind) {
      kind = value;
    },

    isDownloaded(fileName: string): boolean {
      return downloadedFiles.has(fileName);
    },

    async search() {
      const q = query.trim();
      if (!q) return;
      searching = true;
      error = null;
      try {
        results = await invoke<CatalogBrowseItem[]>("browse_civitai", { query: q, kind });
        searched = true;
      } catch (err) {
        error = String(err);
      } finally {
        searching = false;
      }
    },

    async download(item: CatalogBrowseItem) {
      if (downloadingFile) return;
      downloadingFile = item.file_name;
      progressLine = "";
      error = null;

      let unlisten: UnlistenFn | null = null;
      try {
        unlisten = await listen<DownloadEvent>("civitai-download-progress", (e) => {
          if (e.payload.file !== item.file_name) return;
          if (e.payload.type === "step" && e.payload.name) progressLine = e.payload.name;
          if (e.payload.type === "output" && e.payload.line) progressLine = e.payload.line;
        });
        await invoke("download_civitai_model", {
          kind: item.kind,
          fileName: item.file_name,
          downloadUrl: item.download_url,
        });
        downloadedFiles = new Set([...downloadedFiles, item.file_name]);
      } catch (err) {
        error = String(err);
      } finally {
        unlisten?.();
        downloadingFile = null;
        progressLine = "";
      }
    },
  };
}

export const civitaiBrowserStore = createCivitaiBrowserStore();
