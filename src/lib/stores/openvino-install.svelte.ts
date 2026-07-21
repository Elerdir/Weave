import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { notify } from "$lib/services/notify";

export interface OpenvinoDeviceCheck {
  devices: string[];
  hasNpu: boolean;
  openvino: string;
}

export interface OpenvinoRuntimeStatus {
  installed: boolean;
  serverRunning: boolean;
  installDir: string;
  pythonPath: string;
  requirementsPath: string;
  serverLogPath: string;
  defaultModelDir: string;
  /** Naposledy použitá složka modelu — přežije restart aplikace. */
  savedModelDir: string;
  /** Co OpenVINO při instalaci našlo za zařízení; null = zatím neověřeno. */
  deviceCheck: OpenvinoDeviceCheck | null;
}

export interface OpenvinoModelProfile {
  id: string;
  name: string;
  description: string;
  targetDir: string;
  repoId: string | null;
  sourceUrl: string | null;
  autoDownloadable: boolean;
  sizeHint: string;
  qualityTier: string;
}

interface InstallEvent {
  type: "step" | "output" | "done" | "error";
  name?: string;
  line?: string;
  message?: string;
}

function createOpenvinoInstallStore() {
  let status = $state<OpenvinoRuntimeStatus | null>(null);
  let installing = $state(false);
  let uninstalling = $state(false);
  let currentStep = $state("");
  let log = $state<string[]>([]);
  let error = $state<string | null>(null);
  let modelDir = $state("");
  let profiles = $state<OpenvinoModelProfile[]>([]);
  let selectedProfileId = $state("qwen3-8b-int4-cw-ov");
  let startingServer = $state(false);
  let stoppingServer = $state(false);
  let downloadingModel = $state(false);

  return {
    get status() {
      return status;
    },
    get installing() {
      return installing;
    },
    get uninstalling() {
      return uninstalling;
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
    get modelDir() {
      return modelDir;
    },
    get profiles() {
      return profiles;
    },
    get selectedProfileId() {
      return selectedProfileId;
    },
    get selectedProfile() {
      return profiles.find((profile) => profile.id === selectedProfileId) ?? profiles[0] ?? null;
    },
    get startingServer() {
      return startingServer;
    },
    get stoppingServer() {
      return stoppingServer;
    },
    get downloadingModel() {
      return downloadingModel;
    },

    /**
     * Runtime je nainstalovaný, ale OpenVINO na tomhle stroji žádné NPU nevidí
     * — server by se nespustil, takže to má UI říct dopředu.
     */
    get npuMissing() {
      return status?.installed === true && status.deviceCheck?.hasNpu === false;
    },

    get availableDevices() {
      return status?.deviceCheck?.devices ?? [];
    },

    async load() {
      const [nextStatus, nextProfiles] = await Promise.all([
        invoke<OpenvinoRuntimeStatus>("get_openvino_runtime_status"),
        invoke<OpenvinoModelProfile[]>("list_openvino_model_profiles"),
      ]);
      status = nextStatus;
      profiles = nextProfiles;
      if (!profiles.some((profile) => profile.id === selectedProfileId)) {
        selectedProfileId = profiles[0]?.id ?? "qwen3-8b-int4-cw-ov";
      }
      const selected = profiles.find((profile) => profile.id === selectedProfileId);
      if (!modelDir) {
        // Uložená volba má přednost před defaultem profilu — jinak by se po
        // restartu appky ztratila ručně vybraná složka.
        modelDir = status.savedModelDir || selected?.targetDir || status.defaultModelDir;
      }
    },

    setModelDir(value: string) {
      modelDir = value;
    },

    setSelectedProfile(value: string) {
      selectedProfileId = value;
      const selected = profiles.find((profile) => profile.id === value);
      if (selected) modelDir = selected.targetDir;
    },

    async install() {
      if (installing) return;
      installing = true;
      error = null;
      currentStep = "";
      log = [];

      const unlisten = await listen<InstallEvent>("openvino-install-progress", (e) => {
        const ev = e.payload;
        if (ev.type === "step") {
          currentStep = ev.name ?? "";
          log = [...log, `> ${ev.name}`];
        } else if (ev.type === "output") {
          log = [...log.slice(-200), ev.line ?? ""];
        } else if (ev.type === "done") {
          installing = false;
          currentStep = "";
          unlisten();
        } else if (ev.type === "error") {
          error = ev.message ?? "Instalace OpenVINO selhala";
          installing = false;
          unlisten();
        }
      });

      try {
        status = await invoke<OpenvinoRuntimeStatus>("install_openvino_runtime");
        void notify("OpenVINO runtime nainstalovan", "NPU runtime je pripraven k dalsimu nastaveni.");
      } catch (err) {
        error = String(err);
      } finally {
        installing = false;
        currentStep = "";
        unlisten();
        await this.load().catch(() => undefined);
      }
    },

    async uninstall() {
      if (installing || uninstalling) return;
      uninstalling = true;
      error = null;
      try {
        await invoke("uninstall_openvino_runtime");
        await this.load();
        log = [];
        currentStep = "";
      } catch (err) {
        error = String(err);
      } finally {
        uninstalling = false;
      }
    },

    async startServer() {
      if (startingServer) return;
      startingServer = true;
      error = null;
      try {
        status = await invoke<OpenvinoRuntimeStatus>("start_openvino_runtime_server", {
          modelDir: modelDir.trim(),
        });
        void notify("OpenVINO NPU server bezi", "Weave se muze pripojit na http://localhost:8091.");
      } catch (err) {
        error = String(err);
      } finally {
        startingServer = false;
        await this.load().catch(() => undefined);
      }
    },

    async stopServer() {
      if (stoppingServer) return;
      stoppingServer = true;
      error = null;
      try {
        status = await invoke<OpenvinoRuntimeStatus>("stop_openvino_runtime_server");
      } catch (err) {
        error = String(err);
      } finally {
        stoppingServer = false;
        await this.load().catch(() => undefined);
      }
    },

    async downloadRecommendedModel() {
      if (downloadingModel) return;
      downloadingModel = true;
      error = null;
      try {
        const selected = profiles.find((profile) => profile.id === selectedProfileId);
        const profileId = selected?.id ?? "qwen3-8b-int4-cw-ov";
        status = await invoke<OpenvinoRuntimeStatus>("download_openvino_model_profile", {
          profileId,
        });
        modelDir = selected?.targetDir || status.defaultModelDir;
        void notify("OpenVINO model stazen", `${selected?.name ?? "Doporuceny model"} je pripraven pro NPU server.`);
      } catch (err) {
        error = String(err);
      } finally {
        downloadingModel = false;
        await this.load().catch(() => undefined);
      }
    },
  };
}

export const openvinoInstallStore = createOpenvinoInstallStore();
