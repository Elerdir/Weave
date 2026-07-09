import { invoke } from "@tauri-apps/api/core";

/** Tvar odpovídá CatalogModel/CatalogFile v weave-application (snake_case). */
export interface CatalogModel {
  repo_id: string;
  author: string;
  name: string;
  downloads: number;
  likes: number;
  gated: boolean;
}

export interface CatalogFile {
  file_name: string;
  size_bytes: number;
  quant: string | null;
  download_url: string;
}

/**
 * Id lokálního modelu ze jména GGUF souboru: manifest ukládá `{id}.gguf`,
 * takže id = stem souboru očištěný na bezpečné znaky (kvantizace v názvu
 * drží jednotlivé varianty od sebe).
 */
export function modelIdForFile(fileName: string): string {
  const stem = fileName.replace(/\.gguf$/i, "").split("/").pop() ?? fileName;
  return stem.replace(/[^A-Za-z0-9._-]+/g, "-");
}

/** Vyhledávání GGUF modelů na HuggingFace Hub (Nastavení → AI model). */
function createModelSearchStore() {
  let query = $state("");
  let results = $state<CatalogModel[]>([]);
  let searched = $state(false);
  let searching = $state(false);
  /** GGUF soubory po repech — cache, ať rozbalení podruhé nefetchuje. */
  let filesByRepo = $state<Record<string, CatalogFile[]>>({});
  let expandedRepo = $state<string | null>(null);
  let loadingFiles = $state(false);
  let error = $state<string | null>(null);

  return {
    get query() {
      return query;
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
    get expandedRepo() {
      return expandedRepo;
    },
    get loadingFiles() {
      return loadingFiles;
    },
    get error() {
      return error;
    },

    setQuery(value: string) {
      query = value;
    },

    filesFor(repoId: string): CatalogFile[] {
      return filesByRepo[repoId] ?? [];
    },

    async search() {
      const q = query.trim();
      if (!q) return;
      searching = true;
      error = null;
      expandedRepo = null;
      try {
        results = await invoke<CatalogModel[]>("search_model_catalog", { query: q });
        searched = true;
      } catch (err) {
        error = String(err);
      } finally {
        searching = false;
      }
    },

    /** Rozbalí/sbalí repo; při prvním rozbalení dotáhne seznam kvantizací. */
    async toggleRepo(repoId: string) {
      if (expandedRepo === repoId) {
        expandedRepo = null;
        return;
      }
      expandedRepo = repoId;
      if (filesByRepo[repoId]) return;
      loadingFiles = true;
      error = null;
      try {
        const files = await invoke<CatalogFile[]>("list_catalog_gguf_files", { repoId });
        filesByRepo = { ...filesByRepo, [repoId]: files };
      } catch (err) {
        error = String(err);
        expandedRepo = null;
      } finally {
        loadingFiles = false;
      }
    },
  };
}

export const modelSearchStore = createModelSearchStore();
