import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { conversationStore } from "$lib/stores/conversations.svelte";
import type { Conversation } from "$lib/stores/conversations.svelte";

const mockInvoke = vi.mocked(invoke);

function conv(id: string, over: Partial<Conversation> = {}): Conversation {
  return {
    id,
    title: `Konverzace ${id}`,
    persona_id: null,
    pinned: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...over,
  };
}

async function seed(list: Conversation[]) {
  mockInvoke.mockResolvedValueOnce(list);
  await conversationStore.loadAll();
}

describe("conversationStore rename/togglePin", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("rename() ořízne a aktualizuje název", async () => {
    await seed([conv("a")]);
    mockInvoke.mockResolvedValueOnce(undefined);

    await conversationStore.rename("a", "  Nový název  ");

    expect(mockInvoke).toHaveBeenCalledWith("rename_conversation", {
      id: "a",
      title: "Nový název",
    });
    expect(conversationStore.conversations.find((c) => c.id === "a")?.title).toBe("Nový název");
  });

  it("rename() ignoruje prázdný název", async () => {
    await seed([conv("a", { title: "Původní" })]);
    await conversationStore.rename("a", "   ");
    expect(mockInvoke).not.toHaveBeenCalledWith("rename_conversation", expect.anything());
    expect(conversationStore.conversations.find((c) => c.id === "a")?.title).toBe("Původní");
  });

  it("togglePin() připne a přeřadí nahoru", async () => {
    await seed([
      conv("a", { updated_at: "2026-01-01T00:00:00Z" }),
      conv("b", { updated_at: "2026-01-02T00:00:00Z" }),
    ]);
    mockInvoke.mockResolvedValueOnce(undefined);

    await conversationStore.togglePin("a");

    expect(mockInvoke).toHaveBeenCalledWith("set_conversation_pinned", { id: "a", pinned: true });
    // Připnutá "a" musí být první i přes starší updated_at
    expect(conversationStore.conversations[0].id).toBe("a");
    expect(conversationStore.conversations[0].pinned).toBe(true);
  });

  it("pushUserMessage() přidá zprávu bez příloh podle výchozí hodnoty", async () => {
    await seed([conv("a")]);
    mockInvoke.mockResolvedValueOnce([]);
    await conversationStore.select("a");

    conversationStore.pushUserMessage("ahoj");

    const last = conversationStore.messages.at(-1);
    expect(last?.role).toBe("user");
    expect(last?.content).toBe("ahoj");
    expect(last?.attachments).toEqual([]);
  });

  it("pushUserMessage() uloží obrázkové přílohy", async () => {
    await seed([conv("a")]);
    mockInvoke.mockResolvedValueOnce([]);
    await conversationStore.select("a");

    conversationStore.pushUserMessage("podívej se na tohle", [
      { type: "image", path: "/tmp/ref.png", mime: "image/*" },
    ]);

    const last = conversationStore.messages.at(-1);
    expect(last?.attachments).toEqual([
      { type: "image", path: "/tmp/ref.png", mime: "image/*" },
    ]);
  });
});
