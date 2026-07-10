import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  generationSettingsStore,
  DEFAULT_CONTEXT,
  DEFAULT_TEMPERATURE,
  DEFAULT_PULID_WEIGHT,
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
      pulid_weight: null,
      face_detailer: null,
    });

    await generationSettingsStore.load("conv-1");

    expect(mockInvoke).toHaveBeenCalledWith("get_conversation_settings", {
      conversationId: "conv-1",
    });
    expect(generationSettingsStore.contextLength).toBe(DEFAULT_CONTEXT);
    expect(generationSettingsStore.temperature).toBe(DEFAULT_TEMPERATURE);
    expect(generationSettingsStore.maxTokens).toBe(0);
    expect(generationSettingsStore.pulidWeight).toBe(DEFAULT_PULID_WEIGHT);
    expect(generationSettingsStore.faceDetailer).toBe(false);
  });

  it("load() převezme uložené hodnoty", async () => {
    mockInvoke.mockResolvedValueOnce({
      context_length: 16384,
      temperature: 1.2,
      max_tokens: 2048,
      pulid_weight: 0.8,
      face_detailer: true,
    });

    await generationSettingsStore.load("conv-1");

    expect(generationSettingsStore.contextLength).toBe(16384);
    expect(generationSettingsStore.temperature).toBe(1.2);
    expect(generationSettingsStore.maxTokens).toBe(2048);
    expect(generationSettingsStore.pulidWeight).toBe(0.8);
    expect(generationSettingsStore.faceDetailer).toBe(true);
  });

  it("save() posílá maxTokens 0 jako null a věrnostní parametry", async () => {
    mockInvoke.mockResolvedValueOnce({
      context_length: null,
      temperature: null,
      max_tokens: null,
      pulid_weight: null,
      face_detailer: null,
      runtime_backend: null,
      image_checkpoint: null,
      image_lora: null,
    });
    await generationSettingsStore.load("conv-1");

    generationSettingsStore.setContextLength(12288);
    generationSettingsStore.setTemperature(1.05);
    generationSettingsStore.setMaxTokens(0);
    generationSettingsStore.setPulidWeight(0.75);
    generationSettingsStore.setFaceDetailer(true);
    generationSettingsStore.setImageCheckpoint("realvis_ultra.safetensors");
    generationSettingsStore.setImageLora("nikol_v1.safetensors");

    mockInvoke.mockResolvedValueOnce(undefined);
    await generationSettingsStore.save();

    expect(mockInvoke).toHaveBeenLastCalledWith("set_conversation_settings", {
      conversationId: "conv-1",
      settings: {
        context_length: 12288,
        temperature: 1.05,
        max_tokens: null,
        pulid_weight: 0.75,
        face_detailer: true,
        runtime_backend: "default",
        image_checkpoint: "realvis_ultra.safetensors",
        image_lora: "nikol_v1.safetensors",
      },
    });
  });

  it("save() posílá prázdný checkpoint (automatika) jako null", async () => {
    mockInvoke.mockResolvedValueOnce({
      context_length: null,
      temperature: null,
      max_tokens: null,
      pulid_weight: null,
      face_detailer: null,
      runtime_backend: null,
      image_checkpoint: null,
      image_lora: null,
    });
    await generationSettingsStore.load("conv-2");
    generationSettingsStore.setImageCheckpoint("  ");
    generationSettingsStore.setImageLora("");

    mockInvoke.mockResolvedValueOnce(undefined);
    await generationSettingsStore.save();

    const lastCall = mockInvoke.mock.calls.at(-1);
    expect(lastCall?.[0]).toBe("set_conversation_settings");
    expect(
      (lastCall?.[1] as { settings: { image_checkpoint: string | null } }).settings
        .image_checkpoint
    ).toBeNull();
  });

  it("setTemperature() zaokrouhluje float artefakty posuvníku", () => {
    generationSettingsStore.setTemperature(0.7000000000000001);
    expect(generationSettingsStore.temperature).toBe(0.7);
  });

  it("setPulidWeight() zaokrouhluje float artefakty posuvníku", () => {
    generationSettingsStore.setPulidWeight(0.8500000000000001);
    expect(generationSettingsStore.pulidWeight).toBe(0.85);
  });
});
