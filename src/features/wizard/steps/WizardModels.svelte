<script lang="ts">
  import { i18n } from "$lib/i18n/index.svelte";

  // Doporučené modely k stažení — produkčně načíst z API
  const recommended = {
    id: "mistral-7b-instruct-v0.3",
    name: "Mistral 7B Instruct v0.3",
    size: "4.1 GB",
    description: "Rychlý, kvalitní lokální model. Funguje bez internetu.",
  };

  let downloading = $state(false);
  let skipped = $state(false);

  function skip() {
    skipped = true;
  }
</script>

<div class="step">
  <h2>{i18n.m.wizard.steps.models.title}</h2>
  <p class="description">{i18n.m.wizard.steps.models.description}</p>

  {#if !skipped}
    <div class="model-card">
      <div class="model-info">
        <div class="model-name">{recommended.name}</div>
        <div class="model-desc">{recommended.description}</div>
      </div>
      <div class="model-actions">
        <button class="btn-download" onclick={() => downloading = true} disabled={downloading}>
          {downloading
            ? i18n.m.wizard.steps.models.downloading
            : i18n.t("wizard.steps.models.download", { size: recommended.size })}
        </button>
        <button class="btn-skip" onclick={skip}>{i18n.m.wizard.steps.models.skip}</button>
      </div>
      {#if downloading}
        <div class="progress-bar">
          <div class="progress-fill" style="width: 35%"></div>
        </div>
        <span class="progress-text">Stahuji... 35%</span>
      {/if}
    </div>
  {:else}
    <div class="skipped-note">
      Lokální model přeskočen. Weave bude používat Mistral API. Modely lze stáhnout kdykoliv v nastavení.
    </div>
  {/if}
</div>

<style>
  .step { display: flex; flex-direction: column; gap: 1rem; }
  h2 { font-size: 1.25rem; font-weight: 600; }
  .description { color: var(--color-text-muted); line-height: 1.7; font-size: 0.875rem; }

  .model-card {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 12px;
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
    margin-top: 0.5rem;
  }

  .model-name { font-weight: 600; margin-bottom: 0.25rem; }
  .model-desc { font-size: 0.82rem; color: var(--color-text-muted); }

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
  }

  .btn-download:hover:not(:disabled) { background: var(--color-accent-hover); }
  .btn-download:disabled { opacity: 0.6; cursor: default; }

  .btn-skip {
    background: transparent;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.5rem 1rem;
    font-size: 0.875rem;
    cursor: pointer;
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
