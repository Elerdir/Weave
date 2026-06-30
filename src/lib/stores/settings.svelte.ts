import { invoke } from "@tauri-apps/api/core";

export type ApiServiceId = "mistral" | "civitai" | "huggingface";

export interface ApiKeyState {
  service: ApiServiceId;
  hasKey: boolean;
  masked: string | null;
}

const COMFYUI_URL_KEY = "comfyui.url";
const DEFAULT_COMFYUI_URL = "http://localhost:8188";

const SERVICES: ApiServiceId[] = ["mistral", "civitai", "huggingface"];

function createSettingsStore() {
  let apiKeys = $state<Record<ApiServiceId, ApiKeyState>>({
    mistral: { service: "mistral", hasKey: false, masked: null },
    civitai: { service: "civitai", hasKey: false, masked: null },
    huggingface: { service: "huggingface", hasKey: false, masked: null },
  });

  let comfyuiUrl = $state(DEFAULT_COMFYUI_URL);
  let comfyuiStatus = $state<"unknown" | "testing" | "connected" | "disconnected">("unknown");

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

    async load() {
      await Promise.all(SERVICES.map(refreshKey));
      const stored = await invoke<string | null>("get_app_setting", { key: COMFYUI_URL_KEY });
      comfyuiUrl = stored ?? DEFAULT_COMFYUI_URL;
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
