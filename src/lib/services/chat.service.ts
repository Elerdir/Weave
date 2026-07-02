import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Attachment, GenerationStats } from "$lib/stores/conversations.svelte";
import { conversationStore } from "$lib/stores/conversations.svelte";

type StreamChunk =
  | { Token: string }
  | { Done: GenerationStats }
  | { Error: string };

const UNKNOWN_STATS: GenerationStats = {
  tokens_per_second: 0,
  prompt_tokens: 0,
  completion_tokens: 0,
  model_id: "unknown",
  backend: "unknown",
};

/** Přihlásí odběr stream eventů a plní jimi conversation store. */
async function listenForStream(): Promise<() => void> {
  const unlisten = await listen<StreamChunk>("stream-chunk", (event) => {
    const chunk = event.payload;
    if ("Token" in chunk) {
      conversationStore.appendStreamToken(chunk.Token);
    } else if ("Done" in chunk) {
      conversationStore.finalizeStream(chunk.Done);
      unlisten();
    } else if ("Error" in chunk) {
      console.error("Stream error:", chunk.Error);
      conversationStore.setLastError(chunk.Error);
      conversationStore.finalizeStream(UNKNOWN_STATS);
      unlisten();
    }
  });
  return unlisten;
}

/** Spustí backend command generování a při chybě uklidí stav streamu. */
async function runGeneration(
  command: string,
  args: Record<string, unknown>
): Promise<void> {
  conversationStore.startLoading();
  const unlisten = await listenForStream();
  try {
    await invoke(command, args);
  } catch (err) {
    unlisten();
    conversationStore.setLastError(String(err));
    conversationStore.finalizeStream(UNKNOWN_STATS);
    throw err;
  }
}

export async function sendMessage(
  conversationId: string,
  content: string,
  fileRefs: string[] = [],
  referenceImages: string[] = []
): Promise<void> {
  const attachments: Attachment[] = referenceImages.map((path) => ({
    type: "image",
    path,
    mime: "image/*",
  }));
  conversationStore.pushUserMessage(content, attachments);
  await runGeneration("send_message", {
    conversationId,
    content,
    fileRefs,
    referenceImages,
  });
}

/** Znovu vygeneruje poslední odpověď asistenta v konverzaci. */
export async function regenerateResponse(conversationId: string): Promise<void> {
  conversationStore.trimTrailingAssistantMessages();
  await runGeneration("regenerate_response", { conversationId });
}

/** Zastaví právě běžící generování — částečná odpověď zůstane zachovaná. */
export async function stopGeneration(): Promise<void> {
  await invoke("stop_generation");
}
