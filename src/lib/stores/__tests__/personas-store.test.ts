import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { personaStore } from "$lib/stores/personas.svelte";

const mockInvoke = vi.mocked(invoke);

const builtins = [
  { id: "builtin:assistant", name: "Asistent", icon: "🤖", system_prompt: "…", builtin: true },
  { id: "builtin:coder", name: "Kodér", icon: "💻", system_prompt: "…", builtin: true },
];

describe("personaStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("load() načte persony", async () => {
    mockInvoke.mockResolvedValueOnce(builtins);
    await personaStore.load();
    expect(personaStore.personas).toHaveLength(2);
    expect(personaStore.byId("builtin:coder")?.name).toBe("Kodér");
  });

  it("byId() vrátí null pro null/neznámé", async () => {
    mockInvoke.mockResolvedValueOnce(builtins);
    await personaStore.load();
    expect(personaStore.byId(null)).toBeNull();
    expect(personaStore.byId("neznámé")).toBeNull();
  });

  it("create() přidá vlastní personu a ořízne vstupy", async () => {
    mockInvoke.mockResolvedValueOnce(builtins);
    await personaStore.load();

    const created = {
      id: "custom:1",
      name: "Můj",
      icon: "🧪",
      system_prompt: "prompt",
      builtin: false,
    };
    mockInvoke.mockResolvedValueOnce(created);

    await personaStore.create("  Můj  ", "🧪", "  prompt  ");

    expect(mockInvoke).toHaveBeenCalledWith("create_persona", {
      name: "Můj",
      icon: "🧪",
      systemPrompt: "prompt",
    });
    expect(personaStore.personas.some((p) => p.id === "custom:1")).toBe(true);
  });

  it("create() použije výchozí ikonu při prázdné", async () => {
    mockInvoke.mockResolvedValueOnce([]);
    await personaStore.load();
    mockInvoke.mockResolvedValueOnce({ id: "custom:2", name: "X", icon: "🎭", system_prompt: "p", builtin: false });
    await personaStore.create("X", "  ", "p");
    expect(mockInvoke).toHaveBeenCalledWith("create_persona", {
      name: "X",
      icon: "🎭",
      systemPrompt: "p",
    });
  });

  it("remove() odebere personu", async () => {
    mockInvoke.mockResolvedValueOnce([
      { id: "custom:9", name: "Del", icon: "🗑", system_prompt: "p", builtin: false },
    ]);
    await personaStore.load();
    expect(personaStore.personas).toHaveLength(1);

    mockInvoke.mockResolvedValueOnce(undefined);
    await personaStore.remove("custom:9");
    expect(personaStore.personas).toHaveLength(0);
  });
});
