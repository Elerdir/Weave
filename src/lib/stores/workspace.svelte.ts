import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface WorkspaceEntry {
  path: string;
  name: string;
  kind: "file" | "directory";
  size_bytes: number | null;
  modified_at: string | null;
}

export interface IndexedFile {
  path: string;
  name: string;
  extension: string | null;
  size_bytes: number;
  modified_at: string;
  indexed_at: string;
  text_content: string;
}

export interface IndexProgress {
  type: "started" | "file" | "done" | "error";
  total?: number;
  indexed?: number;
  skipped?: number;
  path?: string;
  message?: string;
}

function createWorkspaceStore() {
  let root = $state<string | null>(null);
  let tree = $state<WorkspaceEntry[]>([]);
  let openFile = $state<{ path: string; content: string; dirty: boolean } | null>(null);
  let indexing = $state(false);
  let indexProgress = $state<IndexProgress | null>(null);
  let searchResults = $state<IndexedFile[]>([]);
  let searchQuery = $state("");

  return {
    get root() { return root; },
    get tree() { return tree; },
    get openFile() { return openFile; },
    get indexing() { return indexing; },
    get indexProgress() { return indexProgress; },
    get searchResults() { return searchResults; },
    get searchQuery() { return searchQuery; },

    async load() {
      root = await invoke<string | null>("get_workspace");
      if (root) {
        tree = await invoke<WorkspaceEntry[]>("list_workspace_children", { path: root });
      }
    },

    async setRoot(path: string) {
      await invoke("set_workspace", { path });
      root = path;
      tree = await invoke<WorkspaceEntry[]>("list_workspace_children", { path });
    },

    async expandDir(path: string): Promise<WorkspaceEntry[]> {
      return invoke<WorkspaceEntry[]>("list_workspace_children", { path });
    },

    async openFileAt(path: string) {
      const content = await invoke<string>("read_workspace_file", { path });
      openFile = { path, content, dirty: false };
    },

    updateOpenContent(content: string) {
      if (openFile) {
        openFile = { ...openFile, content, dirty: true };
      }
    },

    async saveOpenFile() {
      if (!openFile || !openFile.dirty) return;
      await invoke("write_workspace_file", { path: openFile.path, content: openFile.content });
      openFile = { ...openFile, dirty: false };
    },

    async createEntry(path: string, isDir: boolean) {
      await invoke("create_workspace_entry", { path, isDir });
      if (root) {
        tree = await invoke<WorkspaceEntry[]>("list_workspace_children", { path: root });
      }
    },

    async deleteEntry(path: string) {
      await invoke("delete_workspace_entry", { path });
      if (openFile?.path === path) openFile = null;
      if (root) {
        tree = await invoke<WorkspaceEntry[]>("list_workspace_children", { path: root });
      }
    },

    async renameEntry(from: string, to: string) {
      await invoke("rename_workspace_entry", { from, to });
      if (openFile?.path === from) {
        openFile = { ...openFile, path: to };
      }
      if (root) {
        tree = await invoke<WorkspaceEntry[]>("list_workspace_children", { path: root });
      }
    },

    async startIndex() {
      if (!root || indexing) return;
      indexing = true;
      indexProgress = null;

      const unlisten = await listen<IndexProgress>("workspace-index-progress", (e) => {
        indexProgress = e.payload;
        if (e.payload.type === "done" || e.payload.type === "error") {
          indexing = false;
          unlisten();
        }
      });

      try {
        await invoke("index_workspace", { path: root });
      } catch (err) {
        indexing = false;
        unlisten();
        throw err;
      }
    },

    async search(query: string) {
      searchQuery = query;
      if (!query.trim()) {
        searchResults = [];
        return;
      }
      searchResults = await invoke<IndexedFile[]>("search_workspace", { query, limit: 20 });
    },

    closeFile() {
      openFile = null;
    },
  };
}

export const workspaceStore = createWorkspaceStore();
