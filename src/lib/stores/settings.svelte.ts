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

export interface NpuInfo {
  available: boolean;
  name: string | null;
  manufacturer: string | null;
  device_id: string | null;
}

const COMFYUI_URL_KEY = "comfyui.url";
const DEFAULT_COMFYUI_URL = "http://localhost:8199";
const LEGACY_COMFYUI_URL = "http://localhost:8188";
const LLM_BACKEND_KEY = "llm.backend";
const LLM_LOCAL_URL_KEY = "llm.local_url";
const DEFAULT_LOCAL_URL = "http://localhost:8080";
const LLM_OPENVINO_NPU_URL_KEY = "llm.openvino_npu_url";
const DEFAULT_OPENVINO_NPU_URL = "http://localhost:8091";
const LLM_MODEL_PATH_KEY = "llm.model_path";
const LLM_GPU_LAYERS_KEY = "llm.gpu_layers";
const DEFAULT_GPU_LAYERS = "999"; // "všechny vrstvy na GPU"
const LLM_CTX_KEY = "llm.context_length";
const DEFAULT_LLM_CTX = "8192"; // musí odpovídat DEFAULT_LLM_CTX v settings.rs
const NOTIFICATIONS_KEY = "notifications.enabled";

export type LlmBackend = "mistral" | "local" | "embedded" | "openvino_npu";
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
  let openvinoNpuUrl = $state(DEFAULT_OPENVINO_NPU_URL);
  let openvinoNpuStatus = $state<ConnStatus>("unknown");
  let npuInfo = $state<NpuInfo | null>(null);
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
    get localUrl() {
      return localUrl;
    },
    get localStatus() {
      return localStatus;
    },
    get openvinoNpuUrl() {
      return openvinoNpuUrl;
    },
    get openvinoNpuStatus() {
      return openvinoNpuStatus;
    },
    get npuInfo() {
      return npuInfo;
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
      llmBackend =
        backend === "local"
          ? "local"
          : backend === "embedded"
            ? "embedded"
            : backend === "openvino_npu"
              ? "openvino_npu"
              : "mistral";
      const lurl = await invoke<string | null>("get_app_setting", { key: LLM_LOCAL_URL_KEY });
      localUrl = lurl ?? DEFAULT_LOCAL_URL;
      const npuUrl = await invoke<string | null>("get_app_setting", {
        key: LLM_OPENVINO_NPU_URL_KEY,
      });
      openvinoNpuUrl = npuUrl ?? DEFAULT_OPENVINO_NPU_URL;
      try {
        npuInfo = await invoke<NpuInfo>("detect_npu");
      } catch {
        npuInfo = null;
      }
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

    setOpenvinoNpuUrl(url: string) {
      openvinoNpuUrl = url;
      openvinoNpuStatus = "unknown";
    },

    async saveOpenvinoNpuUrl() {
      await invoke("set_app_setting", {
        key: LLM_OPENVINO_NPU_URL_KEY,
        value: openvinoNpuUrl,
      });
    },

    async testOpenvinoNpu() {
      openvinoNpuStatus = "testing";
      try {
        const ok = await invoke<boolean>("test_openvino_npu_connection", {
          url: openvinoNpuUrl,
        });
        openvinoNpuStatus = ok ? "connected" : "disconnected";
      } catch {
        openvinoNpuStatus = "disconnected";
      }
    },

    async detectNpu() {
      npuInfo = await invoke<NpuInfo>("detect_npu");
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
     * Přepne aktivní model a zároveň dopočítá `gpu_layers` podle skutečně
     * volné VRAM (max. 80 % — viz `recommend_gpu_layers_for_path` na backendu),
     * ať model, co se nevejde, neskončí OOM/nepředvídatelně pomalým částečným
     * GPU offloadem, ale rovnou běží celý v RAM.
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
