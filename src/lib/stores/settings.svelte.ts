import { invoke } from "@tauri-apps/api/core";
import type { ApiServiceId } from "$lib/token-urls";

export type { ApiServiceId };

export interface ApiKeyState {
  service: ApiServiceId;
  hasKey: boolean;
  masked: string | null;
}

interface StoredApiKeyStatus {
  has_key: boolean;
  masked: string | null;
}

const COMFYUI_URL_KEY = "comfyui.url";
const DEFAULT_COMFYUI_URL = "http://localhost:8199";
const LEGACY_COMFYUI_URL = "http://localhost:8188";
const LLM_BACKEND_KEY = "llm.backend";
const LLM_MODEL_PATH_KEY = "llm.model_path";
const LLM_GPU_LAYERS_KEY = "llm.gpu_layers";
const DEFAULT_GPU_LAYERS = "999"; // "všechny vrstvy na GPU"
const LLM_CTX_KEY = "llm.context_length";
const DEFAULT_LLM_CTX = "8192"; // musí odpovídat DEFAULT_LLM_CTX v settings.rs
const NOTIFICATIONS_KEY = "notifications.enabled";

export type LlmBackend = "embedded";
type ConnStatus = "unknown" | "testing" | "connected" | "disconnected";

const SERVICES: ApiServiceId[] = ["civitai", "huggingface"];

function createSettingsStore() {
  let apiKeys = $state<Record<ApiServiceId, ApiKeyState>>({
    civitai: { service: "civitai", hasKey: false, masked: null },
    huggingface: { service: "huggingface", hasKey: false, masked: null },
  });

  let comfyuiUrl = $state(DEFAULT_COMFYUI_URL);
  let comfyuiStatus = $state<ConnStatus>("unknown");

  let llmBackend = $state<LlmBackend>("embedded");
  let modelPath = $state("");
  let gpuLayers = $state(DEFAULT_GPU_LAYERS);
  let contextLength = $state(DEFAULT_LLM_CTX);

  let notificationsEnabled = $state(true);

  async function refreshKey(service: ApiServiceId) {
    const hasKey = await invoke<boolean>("get_api_key_status", { service });
    const masked = hasKey
      ? await invoke<string | null>("get_masked_api_key", { service })
      : null;
    apiKeys[service] = { service, hasKey, masked };
  }

  return {
    get apiKeys() {
      return apiKeys;
    },
    get comfyuiUrl() {
      return comfyuiUrl;
    },
    get comfyuiStatus() {
      return comfyuiStatus;
    },
    get llmBackend() {
      return llmBackend;
    },
    get modelPath() {
      return modelPath;
    },
    get gpuLayers() {
      return gpuLayers;
    },
    get contextLength() {
      return contextLength;
    },
    get notificationsEnabled() {
      return notificationsEnabled;
    },

    async load() {
      await Promise.all(SERVICES.map(refreshKey));
      const comfy = await invoke<string | null>("get_app_setting", { key: COMFYUI_URL_KEY });
      if (!comfy || comfy === LEGACY_COMFYUI_URL) {
        comfyuiUrl = DEFAULT_COMFYUI_URL;
        if (comfy === LEGACY_COMFYUI_URL) {
          await invoke("set_app_setting", { key: COMFYUI_URL_KEY, value: DEFAULT_COMFYUI_URL });
        }
      } else {
        comfyuiUrl = comfy;
      }
      const backend = await invoke<string | null>("get_app_setting", { key: LLM_BACKEND_KEY });
      // Jediný backend je vestavěná inference; starší uložená hodnota
      // (openvino_npu) se tím tiše přemapuje.
      void backend;
      llmBackend = "embedded";
      const mpath = await invoke<string | null>("get_app_setting", { key: LLM_MODEL_PATH_KEY });
      modelPath = mpath ?? "";
      const layers = await invoke<string | null>("get_app_setting", { key: LLM_GPU_LAYERS_KEY });
      gpuLayers = layers ?? DEFAULT_GPU_LAYERS;
      const ctx = await invoke<string | null>("get_app_setting", { key: LLM_CTX_KEY });
      contextLength = ctx ?? DEFAULT_LLM_CTX;
      const notif = await invoke<string | null>("get_app_setting", { key: NOTIFICATIONS_KEY });
      notificationsEnabled = notif !== "false"; // výchozí zapnuto
    },

    async setNotifications(enabled: boolean) {
      notificationsEnabled = enabled;
      await invoke("set_app_setting", { key: NOTIFICATIONS_KEY, value: String(enabled) });
    },

    async setBackend(backend: LlmBackend) {
      llmBackend = backend;
      await invoke("set_app_setting", { key: LLM_BACKEND_KEY, value: backend });
    },

    setModelPath(path: string) {
      modelPath = path;
    },

    async saveModelPath() {
      await invoke("set_app_setting", { key: LLM_MODEL_PATH_KEY, value: modelPath });
    },

    setGpuLayers(layers: string) {
      gpuLayers = layers;
    },

    async saveGpuLayers() {
      await invoke("set_app_setting", { key: LLM_GPU_LAYERS_KEY, value: gpuLayers });
    },

    /**
     * Přepne aktivní model a zároveň si od backendu vyžádá odhad `gpu_layers`
     * (`recommend_gpu_layers_for_path`).
     *
     * Je to jen orientační hodnota pro UI. Skutečné rozložení modelu mezi GPU
     * a RAM se počítá až při jeho načtení — tam je k dispozici GGUF hlavička
     * i volná VRAM všech karet, takže MoE model větší než VRAM neskončí celý
     * v RAM, ale poběží hybridně (experti v RAM, attention na GPU).
     */
    async activateModel(path: string) {
      this.setModelPath(path);
      await this.saveModelPath();
      try {
        const layers = await invoke<number>("recommend_gpu_layers_for_path", { path });
        this.setGpuLayers(String(layers));
        await this.saveGpuLayers();
      } catch (e) {
        console.warn("Doporučení gpu_layers selhalo, ponechávám současnou hodnotu:", e);
      }
    },

    async unloadEmbeddedModel() {
      await invoke("unload_embedded_model");
    },

    setContextLength(value: string) {
      contextLength = value;
    },

    async saveContextLength() {
      await invoke("set_app_setting", { key: LLM_CTX_KEY, value: contextLength });
    },

    async saveKey(service: ApiServiceId, token: string) {
      const status = await invoke<StoredApiKeyStatus>("store_api_key", {
        service,
        token: token.trim(),
      });
      if (!status) {
        await refreshKey(service);
        return;
      }
      apiKeys[service] = {
        service,
        hasKey: status.has_key,
        masked: status.masked,
      };
      if (!status.has_key) {
        await refreshKey(service);
      }
    },

    async deleteKey(service: ApiServiceId) {
      await invoke("delete_api_key", { service });
      await refreshKey(service);
    },

    setComfyuiUrl(url: string) {
      comfyuiUrl = url;
      comfyuiStatus = "unknown";
    },

    async saveComfyuiUrl() {
      await invoke("set_app_setting", { key: COMFYUI_URL_KEY, value: comfyuiUrl });
    },

    async testComfyui() {
      comfyuiStatus = "testing";
      try {
        const ok = await invoke<boolean>("test_comfyui_connection", { url: comfyuiUrl });
        comfyuiStatus = ok ? "connected" : "disconnected";
      } catch {
        comfyuiStatus = "disconnected";
      }
    },
  };
}

export const settingsStore = createSettingsStore();
