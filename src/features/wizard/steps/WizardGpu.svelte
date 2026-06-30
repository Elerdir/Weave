<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";

  interface GpuInfo {
    name: string;
    vram_mb: number;
    backend: "cuda" | "metal" | "vulkan" | "cpu";
  }

  let detecting = $state(true);
  let gpu = $state<GpuInfo | null>(null);

  onMount(async () => {
    try {
      gpu = await invoke<GpuInfo | null>("detect_gpu");
    } finally {
      detecting = false;
    }
  });

  const backendLabel: Record<string, string> = {
    cuda: "CUDA (NVIDIA)",
    metal: "Metal (Apple)",
    vulkan: "Vulkan",
    cpu: "CPU",
  };
</script>

<div class="step">
  <h2>{i18n.m.wizard.steps.gpu.title}</h2>
  <p class="description">{i18n.m.wizard.steps.gpu.description}</p>

  <div class="gpu-card">
    {#if detecting}
      <div class="detecting">
        <span class="spinner"></span>
        {i18n.m.wizard.steps.gpu.detecting}
      </div>
    {:else if gpu}
      <div class="found">
        <div class="gpu-icon">⚡</div>
        <div class="gpu-info">
          <div class="gpu-name">{gpu.name}</div>
          <div class="gpu-meta">
            <span>{i18n.m.wizard.steps.gpu.backend}: <strong>{backendLabel[gpu.backend] ?? gpu.backend}</strong></span>
            {#if gpu.vram_mb > 0}
              <span>{i18n.m.wizard.steps.gpu.vram}: <strong>{Math.round(gpu.vram_mb / 1024)} GB</strong></span>
            {/if}
          </div>
        </div>
      </div>
    {:else}
      <div class="not-found">
        <span>🖥️</span>
        {i18n.m.wizard.steps.gpu.notFound}
      </div>
    {/if}
  </div>
</div>

<style>
  .step { display: flex; flex-direction: column; gap: 1rem; }
  h2 { font-size: 1.25rem; font-weight: 600; }
  .description { color: var(--color-text-muted); line-height: 1.7; font-size: 0.875rem; }

  .gpu-card {
    margin-top: 0.5rem;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 12px;
    padding: 1.25rem;
  }

  .detecting {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    color: var(--color-text-muted);
  }

  .spinner {
    width: 16px;
    height: 16px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin { to { transform: rotate(360deg); } }

  .found {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .gpu-icon { font-size: 2rem; }

  .gpu-name {
    font-weight: 600;
    font-size: 1rem;
    margin-bottom: 0.35rem;
  }

  .gpu-meta {
    display: flex;
    gap: 1rem;
    font-size: 0.82rem;
    color: var(--color-text-muted);
  }

  .gpu-meta strong { color: var(--color-text); }

  .not-found {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    color: var(--color-text-muted);
    font-size: 0.9rem;
  }
</style>
