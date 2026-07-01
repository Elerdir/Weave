export type ApiServiceId = "mistral" | "civitai" | "huggingface";

/** Stránky, kde si uživatel založí účet a vygeneruje API token. */
export const TOKEN_URLS: Record<ApiServiceId, string> = {
  mistral: "https://console.mistral.ai/api-keys",
  civitai: "https://civitai.com/user/account",
  huggingface: "https://huggingface.co/settings/tokens",
};
