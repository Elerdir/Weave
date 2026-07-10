import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { notify } from "$lib/services/notify";

export type ComfyStatus = "NotInstalled" | "Broken" | "Installed" | "Running";

export interface CheckpointInfo {
  file_name: string;
  size_bytes: number;
}

export interface ComfyDiagnostics {
  status: ComfyStatus;
  install_dir: string;
  main_py_exists: boolean;
  requirements_exists: boolean;
  venv_python_exists: boolean;
  pulid_node_exists: boolean;
  impact_pack_exists: boolean;
  server_log_path: string;
  server_log_tail: string;
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
  let uninstalling = $state(false);
  let checkpoints = $state<CheckpointInfo[]>([]);
  let loras = $state<CheckpointInfo[]>([]);
  let diagnostics = $state<ComfyDiagnostics | null>(null);
  let diagnosing = $state(false);

  return {
    get checkpoints() {
      return checkpoints;
    },
    get loras() {
      return loras;
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
    get uninstalling() {
      return uninstalling;
    },
    get diagnostics() {
      return diagnostics;
    },
    get diagnosing() {
      return diagnosing;
    },

    async load() {
      status = await invoke<ComfyStatus>("get_comfyui_status");
      checkpoints = await invoke<CheckpointInfo[]>("list_image_models");
      loras = await invoke<CheckpointInfo[]>("list_lora_models");
    },

    async deleteCheckpoint(fileName: string) {
      await invoke("delete_image_model", { fileName });
      checkpoints = checkpoints.filter((c) => c.file_name !== fileName);
    },

    async diagnose() {
      diagnosing = true;
      error = null;
      try {
        diagnostics = await invoke<ComfyDiagnostics>("diagnose_comfyui");
        status = diagnostics.status;
      } catch (err) {
        error = String(err);
      } finally {
        diagnosing = false;
      }
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

    async uninstall() {
      if (installing || uninstalling) return;
      uninstalling = true;
      error = null;
      try {
        await invoke("uninstall_comfyui");
        status = "NotInstalled";
        checkpoints = [];
        currentStep = "";
        log = [];
        diagnostics = null;
      } catch (err) {
        error = String(err);
      } finally {
        uninstalling = false;
      }
    },
  };
}

export const comfyInstallStore = createComfyInstallStore();
