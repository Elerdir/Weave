import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Attachment, GenerationStats } from "$lib/stores/conversations.svelte";
import { conversationStore } from "$lib/stores/conversations.svelte";

type StreamChunk =
  | { Token: string }
  | { Done: GenerationStats }
  | { Error: string };

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
  conversationStore.startLoading();

  const unlisten = await listen<StreamChunk>("stream-chunk", (event) => {
    const chunk = event.payload;
    if ("Token" in chunk) {
      conversationStore.appendStreamToken(chunk.Token);
    } else if ("Done" in chunk) {
      conversationStore.finalizeStream(chunk.Done);
      unlisten();
    } else if ("Error" in chunk) {
      console.error("Stream error:", chunk.Error);
      conversationStore.finalizeStream({
        tokens_per_second: 0,
        prompt_tokens: 0,
        completion_tokens: 0,
        model_id: "unknown",
        backend: "unknown",
      });
      unlisten();
    }
  });

  try {
    await invoke("send_message", { conversationId, content, fileRefs, referenceImages });
  } catch (err) {
    unlisten();
    conversationStore.finalizeStream({
      tokens_per_second: 0,
      prompt_tokens: 0,
      completion_tokens: 0,
      model_id: "unknown",
      backend: "unknown",
    });
    throw err;
  }
}

/** Zastaví právě běžící generování — částečná odpověď zůstane zachovaná. */
export async function stopGeneration(): Promise<void> {
  await invoke("stop_generation");
}
