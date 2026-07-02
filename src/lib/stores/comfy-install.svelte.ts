import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { notify } from "$lib/services/notify";

export type ComfyStatus = "NotInstalled" | "Installed" | "Running";

export interface CheckpointInfo {
  file_name: string;
  size_bytes: number;
}

interface InstallEvent {
  type: "step" | "output" | "done" | "error";
  name?: string;
  line?: string;
  message?: string;
}

function createComfyInstallStore() {
  let status = $state<ComfyStatus>("NotInstalled");
  let installing = $state(false);
  let currentStep = $state("");
  let log = $state<string[]>([]);
  let error = $state<string | null>(null);
  let starting = $state(false);
  let checkpoints = $state<CheckpointInfo[]>([]);

  return {
    get checkpoints() {
      return checkpoints;
    },
    get status() {
      return status;
    },
    get installing() {
      return installing;
    },
    get currentStep() {
      return currentStep;
    },
    get log() {
      return log;
    },
    get error() {
      return error;
    },
    get starting() {
      return starting;
    },

    async load() {
      status = await invoke<ComfyStatus>("get_comfyui_status");
      checkpoints = await invoke<CheckpointInfo[]>("list_image_models");
    },

    async deleteCheckpoint(fileName: string) {
      await invoke("delete_image_model", { fileName });
      checkpoints = checkpoints.filter((c) => c.file_name !== fileName);
    },

    async install() {
      if (installing) return;
      installing = true;
      error = null;
      log = [];
      currentStep = "";

      const unlisten = await listen<InstallEvent>("comfyui-install-progress", (e) => {
        const ev = e.payload;
        if (ev.type === "step") {
          currentStep = ev.name ?? "";
          log = [...log, `▶ ${ev.name}`];
        } else if (ev.type === "output") {
          log = [...log.slice(-200), ev.line ?? ""];
        } else if (ev.type === "done") {
          installing = false;
          currentStep = "";
          unlisten();
          void this.load();
          void notify("ComfyUI nainstalováno", "Server je připraven ke spuštění.");
        } else if (ev.type === "error") {
          error = ev.message ?? "Instalace selhala";
          installing = false;
          unlisten();
        }
      });

      try {
        await invoke("install_comfyui");
      } catch (err) {
        error = String(err);
        installing = false;
        unlisten();
      }
    },

    async startServer() {
      starting = true;
      error = null;
      try {
        await invoke("start_comfyui_server");
        status = "Running";
      } catch (err) {
        error = String(err);
      } finally {
        starting = false;
      }
    },

    async stopServer() {
      await invoke("stop_comfyui_server");
      status = "Installed";
    },
  };
}

export const comfyInstallStore = createComfyInstallStore();
