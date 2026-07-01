import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { sendMessage } from "$lib/services/chat.service";
import { conversationStore } from "$lib/stores/conversations.svelte";

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

async function activateConversation(id: string) {
  mockInvoke.mockResolvedValueOnce([]);
  await conversationStore.select(id);
}

describe("chat.service sendMessage", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    mockListen.mockResolvedValue(() => {});
    await activateConversation("conv-1");
    vi.clearAllMocks();
    mockListen.mockResolvedValue(() => {});
  });

  it("pushí optimistickou zprávu uživatele s náhledy referenčních obrázků", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await sendMessage("conv-1", "nakresli mě jako rytíře", [], ["/tmp/selfie.png"]);

    const pushed = conversationStore.messages.at(-1);
    expect(pushed?.role).toBe("user");
    expect(pushed?.attachments).toEqual([
      { type: "image", path: "/tmp/selfie.png", mime: "image/*" },
    ]);
  });

  it("zavolá send_message s fileRefs i referenceImages", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await sendMessage("conv-1", "ahoj", ["/ws/note.txt"], ["/tmp/ref.png"]);

    expect(mockInvoke).toHaveBeenCalledWith("send_message", {
      conversationId: "conv-1",
      content: "ahoj",
      fileRefs: ["/ws/note.txt"],
      referenceImages: ["/tmp/ref.png"],
    });
  });

  it("bez obrázků pošle prázdné pole a žádné přílohy", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await sendMessage("conv-1", "ahoj");

    expect(mockInvoke).toHaveBeenCalledWith("send_message", {
      conversationId: "conv-1",
      content: "ahoj",
      fileRefs: [],
      referenceImages: [],
    });
    expect(conversationStore.messages.at(-1)?.attachments).toEqual([]);
  });

  it("zpracuje stream-chunk Token a Done eventy", async () => {
    let handler: ((e: { payload: unknown }) => void) | null = null;
    mockListen.mockImplementation(async (_event, cb) => {
      handler = cb as (e: { payload: unknown }) => void;
      return () => {};
    });
    mockInvoke.mockImplementation(async () => {
      handler?.({ payload: { Token: "Ahoj" } });
      handler?.({
        payload: {
          Done: {
            tokens_per_second: 12.5,
            prompt_tokens: 5,
            completion_tokens: 10,
            model_id: "mistral-small-latest",
            backend: "mistral_api",
          },
        },
      });
      return undefined;
    });

    await sendMessage("conv-1", "ahoj");

    expect(conversationStore.streamingContent).toBeNull();
    expect(conversationStore.currentStats?.model_id).toBe("mistral-small-latest");
    expect(conversationStore.messages.at(-1)?.content).toBe("Ahoj");
  });

  it("při selhání invoke resetuje loading stav a propaguje chybu", async () => {
    mockInvoke.mockRejectedValueOnce(new Error("síť nedostupná"));

    await expect(sendMessage("conv-1", "ahoj")).rejects.toThrow("síť nedostupná");
    expect(conversationStore.loading).toBe(false);
  });
});
