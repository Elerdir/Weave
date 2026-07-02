import { invoke } from "@tauri-apps/api/core";

/** Tvar odpovídá GenerationSettings v weave-domain (snake_case přes serde). */
export interface GenerationSettings {
  context_length: number | null;
  temperature: number | null;
  max_tokens: number | null;
}

export const DEFAULT_CONTEXT = 8192;
export const DEFAULT_TEMPERATURE = 0.7;

/** Parametry generování aktivní konverzace (posuvníky v hlavičce chatu). */
function createGenerationSettingsStore() {
  let conversationId = $state<string | null>(null);
  let contextLength = $state(DEFAULT_CONTEXT);
  let temperature = $state(DEFAULT_TEMPERATURE);
  /** 0 = bez omezení (ukládá se jako null) */
  let maxTokens = $state(0);

  return {
    get contextLength() {
      return contextLength;
    },
    get temperature() {
      return temperature;
    },
    get maxTokens() {
      return maxTokens;
    },

    async load(id: string) {
      conversationId = id;
      const s = await invoke<GenerationSettings>("get_conversation_settings", {
        conversationId: id,
      });
      contextLength = s.context_length ?? DEFAULT_CONTEXT;
      temperature = s.temperature ?? DEFAULT_TEMPERATURE;
      maxTokens = s.max_tokens ?? 0;
    },

    setContextLength(value: number) {
      contextLength = value;
    },

    setTemperature(value: number) {
      // Krok posuvníku 0.05 → zaokrouhlení proti float artefaktům
      temperature = Math.round(value * 100) / 100;
    },

    setMaxTokens(value: number) {
      maxTokens = value;
    },

    async save() {
      if (!conversationId) return;
      const settings: GenerationSettings = {
        context_length: contextLength,
        temperature,
        max_tokens: maxTokens > 0 ? maxTokens : null,
      };
      await invoke("set_conversation_settings", { conversationId, settings });
    },
  };
}

export const generationSettingsStore = createGenerationSettingsStore();
