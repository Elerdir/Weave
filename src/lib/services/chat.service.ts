import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  Attachment,
  GenerationStats,
  ImageStageInfo,
} from "$lib/stores/conversations.svelte";
import { conversationStore } from "$lib/stores/conversations.svelte";

type StreamChunk =
  | { Token: string }
  | { ImageStage: ImageStageInfo }
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
    } else if ("ImageStage" in chunk) {
      conversationStore.setImageStage(chunk.ImageStage);
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
    // Sladí lokální stav s DB — hlavně skutečná ID zpráv (potřebná pro
    // resend/edit, lokálně vytvořené bubliny mají jen dočasná ID).
    await conversationStore.reloadMessages();
    await maybeAutoTitle();
  } catch (err) {
    unlisten();
    conversationStore.setLastError(String(err));
    conversationStore.finalizeStream(UNKNOWN_STATS);
    throw err;
  }
}

/** Výchozí názvy konverzací (cs/en) — jen ty se auto-pojmenovávají. */
const DEFAULT_TITLE_PREFIXES = ["Nová konverzace", "New Conversation"];

/** Po dokončené výměně nechá LLM pojmenovat konverzaci s výchozím názvem.
 *  Best-effort — selhání se jen zaloguje, chat běží dál. */
async function maybeAutoTitle(): Promise<void> {
  const conv = conversationStore.activeConversation;
  if (!conv) return;
  const isDefault = DEFAULT_TITLE_PREFIXES.some((p) => conv.title.startsWith(p));
  const hasExchange =
    conversationStore.messages.some((m) => m.role === "user") &&
    conversationStore.messages.some((m) => m.role === "assistant");
  if (!isDefault || !hasExchange) return;

  try {
    const title = await invoke<string>("auto_title_conversation", {
      conversationId: conv.id,
    });
    conversationStore.updateTitleLocal(conv.id, title);
  } catch (err) {
    console.warn("Auto-pojmenování selhalo:", err);
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

/** Úprava vygenerovaného obrázku (img2img): instrukce + výchozí obrázek.
 *  Backend uloží dotaz s náhledem obrázku a generuje z něj (denoise ~0.55). */
export async function editImageMessage(
  conversationId: string,
  content: string,
  initImage: string
): Promise<void> {
  conversationStore.pushUserMessage(content, [
    { type: "image", path: initImage, mime: "image/*" },
  ]);
  await runGeneration("edit_image_message", {
    conversationId,
    content,
    initImage,
  });
}

/** Znovu vygeneruje poslední odpověď asistenta v konverzaci. */
export async function regenerateResponse(conversationId: string): Promise<void> {
  conversationStore.trimTrailingAssistantMessages();
  await runGeneration("regenerate_response", { conversationId });
}

/** „Poslat znovu": smaže vše po dané zprávě (dotazy i odpovědi) a vygeneruje
 *  čerstvou odpověď na ni. */
export async function resendMessage(
  conversationId: string,
  messageId: string
): Promise<void> {
  conversationStore.truncateAfterLocal(messageId);
  await runGeneration("resend_message", { conversationId, messageId });
}

/** „Upravit a poslat": smaže původní zprávu a vše po ní, pak pošle novou verzi. */
export async function sendEditedMessage(
  conversationId: string,
  messageId: string,
  content: string,
  referenceImages: string[] = []
): Promise<void> {
  conversationStore.truncateFromLocal(messageId);
  await invoke("truncate_conversation_from", { conversationId, messageId });
  await sendMessage(conversationId, content, [], referenceImages);
}

/** Zastaví právě běžící generování — částečná odpověď zůstane zachovaná. */
export async function stopGeneration(): Promise<void> {
  await invoke("stop_generation");
}
