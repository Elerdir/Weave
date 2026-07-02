import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  generationSettingsStore,
  DEFAULT_CONTEXT,
  DEFAULT_TEMPERATURE,
} from "$lib/stores/generation-settings.svelte";

const mockInvoke = vi.mocked(invoke);

describe("generationSettingsStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("load() mapuje null hodnoty na výchozí (max_tokens → 0 = neomezeno)", async () => {
    mockInvoke.mockResolvedValueOnce({
      context_length: null,
      temperature: null,
      max_tokens: null,
    });

    await generationSettingsStore.load("conv-1");

    expect(mockInvoke).toHaveBeenCalledWith("get_conversation_settings", {
      conversationId: "conv-1",
    });
    expect(generationSettingsStore.contextLength).toBe(DEFAULT_CONTEXT);
    expect(generationSettingsStore.temperature).toBe(DEFAULT_TEMPERATURE);
    expect(generationSettingsStore.maxTokens).toBe(0);
  });

  it("load() převezme uložené hodnoty", async () => {
    mockInvoke.mockResolvedValueOnce({
      context_length: 16384,
      temperature: 1.2,
      max_tokens: 2048,
    });

    await generationSettingsStore.load("conv-1");

    expect(generationSettingsStore.contextLength).toBe(16384);
    expect(generationSettingsStore.temperature).toBe(1.2);
    expect(generationSettingsStore.maxTokens).toBe(2048);
  });

  it("save() posílá maxTokens 0 jako null (bez omezení)", async () => {
    mockInvoke.mockResolvedValueOnce({
      context_length: null,
      temperature: null,
      max_tokens: null,
    });
    await generationSettingsStore.load("conv-1");

    generationSettingsStore.setContextLength(12288);
    generationSettingsStore.setTemperature(1.05);
    generationSettingsStore.setMaxTokens(0);

    mockInvoke.mockResolvedValueOnce(undefined);
    await generationSettingsStore.save();

    expect(mockInvoke).toHaveBeenLastCalledWith("set_conversation_settings", {
      conversationId: "conv-1",
      settings: { context_length: 12288, temperature: 1.05, max_tokens: null },
    });
  });

  it("setTemperature() zaokrouhluje float artefakty posuvníku", () => {
    generationSettingsStore.setTemperature(0.7000000000000001);
    expect(generationSettingsStore.temperature).toBe(0.7);
  });
});
