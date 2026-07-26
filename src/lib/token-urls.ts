export type ApiServiceId = "civitai" | "huggingface";

/** Stránky, kde si uživatel založí účet a vygeneruje API token. */
export const TOKEN_URLS: Record<ApiServiceId, string> = {
  civitai: "https://civitai.com/user/account",
  huggingface: "https://huggingface.co/settings/tokens",
};
