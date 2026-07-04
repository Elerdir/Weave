<script lang="ts">
  import { onMount } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import { modelsStore, formatBytes, formatSpeed } from "$lib/stores/models.svelte";

  let skipped = $state(false);

  onMount(() => {
    modelsStore.load().catch((e) => console.warn("models load selhal:", e));
  });

  function downloadPercent(): number {
    const d = modelsStore.download;
    if (!d || d.total === 0) return 0;
    return Math.round((d.downloaded / d.total) * 100);
  }
</script>

<div class="step">
  <h2>{i18n.m.wizard.steps.models.title}</h2>
  <p class="description">{i18n.m.wizard.steps.models.description}</p>

  {#if !skipped}
    {#if modelsStore.recommended.length === 0}
      <p class="loading-hint">{i18n.m.common.loading}</p>
    {:else}
      <div class="model-list">
        {#each modelsStore.recommended as rec (rec.id)}
          {@const downloaded = modelsStore.isDownloaded(rec.id)}
          <div class="model-card">
            <div class="model-info">
              <div class="model-name">
                {rec.name}
                {#if downloaded}<span class="done-badge">✓ {i18n.m.wizard.steps.models.ready}</span>{/if}
              </div>
              <div class="model-desc">{rec.description}</div>
              <div class="model-size">{formatBytes(rec.size_bytes)}</div>
            </div>

            {#if downloaded}
              <!-- Model už stažen, nic dalšího tu není potřeba dělat -->
            {:else if modelsStore.download?.modelId === rec.id}
              <div class="progress-bar">
                <div class="progress-fill" style="width: {downloadPercent()}%"></div>
              </div>
              <span class="progress-text">
                {#if modelsStore.download.phase === "verifying"}
                  {i18n.m.common.loading}
                {:else}
                  {i18n.m.wizard.steps.models.downloading}
                  {downloadPercent()}% ({formatBytes(modelsStore.download.downloaded)} / {formatBytes(modelsStore.download.total)}{#if modelsStore.download.speedBytesPerSec > 0} · {formatSpeed(modelsStore.download.speedBytesPerSec)}{/if})
                {/if}
              </span>
            {:else}
              <div class="model-actions">
                <button
                  class="btn-download"
                  disabled={!!modelsStore.download}
                  onclick={() => modelsStore.downloadRecommended(rec.id)}
                >
                  {i18n.t("wizard.steps.models.download", { size: formatBytes(rec.size_bytes) })}
                </button>
              </div>
            {/if}
          </div>
        {/each}
      </div>

      {#if modelsStore.error}
        <p class="error-text">{modelsStore.error}</p>
      {/if}

      <button class="btn-skip" onclick={() => (skipped = true)}>
        {i18n.m.wizard.steps.models.skip}
      </button>
    {/if}
  {:else}
    <div class="skipped-note">
      {i18n.m.wizard.steps.models.skippedNote}
    </div>
  {/if}
</div>

<style>
  .step { display: flex; flex-direction: column; gap: 1rem; }
  h2 { font-size: 1.25rem; font-weight: 600; }
  .description { color: var(--color-text-muted); line-height: 1.7; font-size: 0.875rem; }
  .loading-hint { color: var(--color-text-muted); font-size: 0.875rem; }

  .model-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    margin-top: 0.5rem;
    max-height: 280px;
    overflow-y: auto;
  }

  .model-card {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 12px;
    padding: 1rem 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .model-name {
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .done-badge {
    font-size: 0.72rem;
    font-weight: 600;
    color: var(--color-success);
  }
  .model-desc { font-size: 0.82rem; color: var(--color-text-muted); }
  .model-size { font-size: 0.72rem; color: var(--color-text-muted); }

  .model-actions {
    display: flex;
    gap: 0.75rem;
  }

  .btn-download {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 8px;
    padding: 0.5rem 1.25rem;
    font-size: 0.875rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
    align-self: flex-start;
  }

  .btn-download:hover:not(:disabled) { background: var(--color-accent-hover); }
  .btn-download:disabled { opacity: 0.5; cursor: default; }

  .btn-skip {
    background: transparent;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.5rem 1rem;
    font-size: 0.875rem;
    cursor: pointer;
    align-self: flex-start;
  }

  .progress-bar {
    height: 6px;
    background: var(--color-border);
    border-radius: 3px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--color-accent);
    border-radius: 3px;
    transition: width 0.3s;
  }

  .progress-text {
    font-size: 0.8rem;
    color: var(--color-text-muted);
  }

  .error-text {
    font-size: 0.8rem;
    color: var(--color-error);
  }

  .skipped-note {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 12px;
    padding: 1rem;
    font-size: 0.875rem;
    color: var(--color-text-muted);
    line-height: 1.7;
  }
</style>
