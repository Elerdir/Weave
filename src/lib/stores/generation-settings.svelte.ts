import { invoke } from "@tauri-apps/api/core";

/** Tvar odpovídá GenerationSettings v weave-domain (snake_case přes serde). */
export interface GenerationSettings {
  context_length: number | null;
  temperature: number | null;
  max_tokens: number | null;
  pulid_weight: number | null;
  face_detailer: boolean | null;
  runtime_backend: RuntimeBackend | null;
  image_checkpoint: string | null;
  image_lora: string | null;
}

export type RuntimeBackend = "default" | "mistral" | "local" | "embedded" | "openvino_npu";

export const DEFAULT_CONTEXT = 8192;
export const DEFAULT_TEMPERATURE = 0.7;
export const DEFAULT_PULID_WEIGHT = 1.0;

/** Parametry generování aktivní konverzace (posuvníky v hlavičce chatu). */
function createGenerationSettingsStore() {
  let conversationId = $state<string | null>(null);
  let contextLength = $state(DEFAULT_CONTEXT);
  let temperature = $state(DEFAULT_TEMPERATURE);
  /** 0 = bez omezení (ukládá se jako null) */
  let maxTokens = $state(0);
  /** Síla PuLID podoby (ApplyPulid weight) — uplatní se u referenčních fotek. */
  let pulidWeight = $state(DEFAULT_PULID_WEIGHT);
  /** Doladit obličej/oči FaceDetailerem (Impact Pack). */
  let faceDetailer = $state(false);
  let runtimeBackend = $state<RuntimeBackend>("default");
  /** Checkpoint obrázků; "" = automaticky podle stylu promptu. */
  let imageCheckpoint = $state("");
  /** LoRA obrázků; "" = automatické hledání na CivitAI podle promptu. */
  let imageLora = $state("");

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
    get pulidWeight() {
      return pulidWeight;
    },
    get faceDetailer() {
      return faceDetailer;
    },
    get runtimeBackend() {
      return runtimeBackend;
    },
    get imageCheckpoint() {
      return imageCheckpoint;
    },
    get imageLora() {
      return imageLora;
    },

    async load(id: string) {
      conversationId = id;
      const s = await invoke<GenerationSettings>("get_conversation_settings", {
        conversationId: id,
      });
      contextLength = s.context_length ?? DEFAULT_CONTEXT;
      temperature = s.temperature ?? DEFAULT_TEMPERATURE;
      maxTokens = s.max_tokens ?? 0;
      pulidWeight = s.pulid_weight ?? DEFAULT_PULID_WEIGHT;
      faceDetailer = s.face_detailer ?? false;
      runtimeBackend = s.runtime_backend ?? "default";
      imageCheckpoint = s.image_checkpoint ?? "";
      imageLora = s.image_lora ?? "";
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

    setPulidWeight(value: number) {
      // Krok 0.05 → zaokrouhlení proti float artefaktům
      pulidWeight = Math.round(value * 100) / 100;
    },

    setFaceDetailer(value: boolean) {
      faceDetailer = value;
    },

    setRuntimeBackend(value: RuntimeBackend) {
      runtimeBackend = value;
    },

    setImageCheckpoint(value: string) {
      imageCheckpoint = value;
    },

    setImageLora(value: string) {
      imageLora = value;
    },

    async save() {
      if (!conversationId) return;
      const settings: GenerationSettings = {
        context_length: contextLength,
        temperature,
        max_tokens: maxTokens > 0 ? maxTokens : null,
        pulid_weight: pulidWeight,
        face_detailer: faceDetailer,
        runtime_backend: runtimeBackend,
        image_checkpoint: imageCheckpoint.trim() ? imageCheckpoint.trim() : null,
        image_lora: imageLora.trim() ? imageLora.trim() : null,
      };
      await invoke("set_conversation_settings", { conversationId, settings });
    },
  };
}

export const generationSettingsStore = createGenerationSettingsStore();
