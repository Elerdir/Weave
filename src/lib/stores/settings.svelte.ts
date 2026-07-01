import { invoke } from "@tauri-apps/api/core";

export type ApiServiceId = "mistral" | "civitai" | "huggingface";

export interface ApiKeyState {
  service: ApiServiceId;
  hasKey: boolean;
  masked: string | null;
}

const COMFYUI_URL_KEY = "comfyui.url";
const DEFAULT_COMFYUI_URL = "http://localhost:8188";
const LLM_BACKEND_KEY = "llm.backend";
const LLM_LOCAL_URL_KEY = "llm.local_url";
const DEFAULT_LOCAL_URL = "http://localhost:8080";
const LLM_MODEL_PATH_KEY = "llm.model_path";
const LLM_GPU_LAYERS_KEY = "llm.gpu_layers";
const DEFAULT_GPU_LAYERS = "999"; // "všechny vrstvy na GPU"
const NOTIFICATIONS_KEY = "notifications.enabled";

export type LlmBackend = "mistral" | "local" | "embedded";
type ConnStatus = "unknown" | "testing" | "connected" | "disconnected";

const SERVICES: ApiServiceId[] = ["mistral", "civitai", "huggingface"];

function createSettingsStore() {
  let apiKeys = $state<Record<ApiServiceId, ApiKeyState>>({
    mistral: { service: "mistral", hasKey: false, masked: null },
    civitai: { service: "civitai", hasKey: false, masked: null },
    huggingface: { service: "huggingface", hasKey: false, masked: null },
  });

  let comfyuiUrl = $state(DEFAULT_COMFYUI_URL);
  let comfyuiStatus = $state<ConnStatus>("unknown");

  let llmBackend = $state<LlmBackend>("mistral");
  let localUrl = $state(DEFAULT_LOCAL_URL);
  let localStatus = $state<ConnStatus>("unknown");
  let modelPath = $state("");
  let gpuLayers = $state(DEFAULT_GPU_LAYERS);

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
    get localUrl() {
      return localUrl;
    },
    get localStatus() {
      return localStatus;
    },
    get modelPath() {
      return modelPath;
    },
    get gpuLayers() {
      return gpuLayers;
    },
    get notificationsEnabled() {
      return notificationsEnabled;
    },

    async load() {
      await Promise.all(SERVICES.map(refreshKey));
      const comfy = await invoke<string | null>("get_app_setting", { key: COMFYUI_URL_KEY });
      comfyuiUrl = comfy ?? DEFAULT_COMFYUI_URL;
      const backend = await invoke<string | null>("get_app_setting", { key: LLM_BACKEND_KEY });
      llmBackend =
        backend === "local" ? "local" : backend === "embedded" ? "embedded" : "mistral";
      const lurl = await invoke<string | null>("get_app_setting", { key: LLM_LOCAL_URL_KEY });
      localUrl = lurl ?? DEFAULT_LOCAL_URL;
      const mpath = await invoke<string | null>("get_app_setting", { key: LLM_MODEL_PATH_KEY });
      modelPath = mpath ?? "";
      const layers = await invoke<string | null>("get_app_setting", { key: LLM_GPU_LAYERS_KEY });
      gpuLayers = layers ?? DEFAULT_GPU_LAYERS;
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

    setLocalUrl(url: string) {
      localUrl = url;
      localStatus = "unknown";
    },

    async saveLocalUrl() {
      await invoke("set_app_setting", { key: LLM_LOCAL_URL_KEY, value: localUrl });
    },

    async testLocal() {
      localStatus = "testing";
      try {
        const ok = await invoke<boolean>("test_local_llm_connection", { url: localUrl });
        localStatus = ok ? "connected" : "disconnected";
      } catch {
        localStatus = "disconnected";
      }
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

    async saveKey(service: ApiServiceId, token: string) {
      await invoke("store_api_key", { service, token: token.trim() });
      await refreshKey(service);
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
