import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { regenerateResponse, sendMessage, stopGeneration } from "$lib/services/chat.service";
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
      referencePreservation: null,
      translateImagePrompt: true,
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
      referencePreservation: null,
      translateImagePrompt: true,
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

  it("Error chunk nastaví viditelnou chybu a další odeslání ji smaže", async () => {
    let handler: ((e: { payload: unknown }) => void) | null = null;
    mockListen.mockImplementation(async (_event, cb) => {
      handler = cb as (e: { payload: unknown }) => void;
      return () => {};
    });
    mockInvoke.mockImplementation(async () => {
      handler?.({ payload: { Error: "Zpráva se nevejde do kontextového okna" } });
      return undefined;
    });

    await sendMessage("conv-1", "dlouhá zpráva");
    expect(conversationStore.lastError).toContain("kontextového okna");
    expect(conversationStore.loading).toBe(false);

    // Nové odeslání chybu vyčistí (reset — jinak by i reload zpráv
    // spadl do mockImplementation výše a vystřelil další Error chunk)
    mockListen.mockResolvedValue(() => {});
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue(undefined);
    await sendMessage("conv-1", "kratší zpráva");
    expect(conversationStore.lastError).toBeNull();
  });

  it("ImageStage chunky plní průběh obrázku a Done ho vyčistí", async () => {
    let handler: ((e: { payload: unknown }) => void) | null = null;
    mockListen.mockImplementation(async (_event, cb) => {
      handler = cb as (e: { payload: unknown }) => void;
      return () => {};
    });
    mockInvoke.mockImplementation(async () => {
      handler?.({
        payload: { ImageStage: { stage: "installing", detail: "pip install torch" } },
      });
      expect(conversationStore.imageStage?.stage).toBe("installing");
      expect(conversationStore.imageStage?.detail).toBe("pip install torch");

      handler?.({ payload: { ImageStage: { stage: "generating", detail: null } } });
      expect(conversationStore.imageStage?.stage).toBe("generating");

      handler?.({ payload: { Token: "![obrázek](/gallery/x.png)" } });
      handler?.({
        payload: {
          Done: {
            tokens_per_second: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            model_id: "comfyui",
            backend: "comfy_ui",
          },
        },
      });
      return undefined;
    });

    await sendMessage("conv-1", "nakresli hrad");

    expect(conversationStore.imageStage).toBeNull();
    expect(conversationStore.messages.at(-1)?.content).toContain("/gallery/x.png");
  });

  it("stopGeneration() zavolá backend command", async () => {
    mockInvoke.mockResolvedValueOnce(undefined);

    await stopGeneration();

    expect(mockInvoke).toHaveBeenCalledWith("stop_generation");
  });

  it("regenerateResponse() odebere poslední odpověď a zavolá backend", async () => {
    // Historie: user zpráva + assistant odpověď (bez nového pushnutí user zprávy).
    // Reset kvůli implementacím z předchozích testů — reload zpráv volá
    // invoke navíc a nesmí do nich spadnout.
    mockInvoke.mockReset();
    mockInvoke.mockResolvedValue(undefined);
    await sendMessage("conv-1", "otázka");
    conversationStore.finalizeStream({
      tokens_per_second: 1,
      prompt_tokens: 1,
      completion_tokens: 1,
      model_id: "m",
      backend: "b",
    });
    conversationStore.appendStreamToken("stará odpověď");
    conversationStore.finalizeStream({
      tokens_per_second: 1,
      prompt_tokens: 1,
      completion_tokens: 1,
      model_id: "m",
      backend: "b",
    });
    const countBefore = conversationStore.messages.length;
    expect(conversationStore.messages.at(-1)?.role).toBe("assistant");

    mockInvoke.mockClear();
    mockInvoke.mockResolvedValue(undefined);
    await regenerateResponse("conv-1");

    expect(mockInvoke).toHaveBeenCalledWith("regenerate_response", {
      conversationId: "conv-1",
    });
    // Poslední assistant zpráva zmizela, žádná nová user zpráva nepřibyla
    expect(conversationStore.messages.length).toBe(countBefore - 1);
    expect(conversationStore.messages.at(-1)?.role).not.toBe("assistant");
  });
});
