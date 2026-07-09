<script lang="ts">
  import { onMount } from "svelte";
  import { open as openUrl } from "@tauri-apps/plugin-shell";
  import { i18n } from "$lib/i18n/index.svelte";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import { openvinoInstallStore } from "$lib/stores/openvino-install.svelte";

  let enabling = $state(false);

  onMount(() => {
    settingsStore.load().catch((e) => console.warn("settings load selhal:", e));
    openvinoInstallStore.load().catch((e) => console.warn("openvino load selhal:", e));
  });

  async function enableNpuBackend() {
    enabling = true;
    try {
      settingsStore.setOpenvinoNpuUrl("http://localhost:8091");
      await settingsStore.saveOpenvinoNpuUrl();
      await settingsStore.setBackend("openvino_npu");
      await settingsStore.testOpenvinoNpu();
    } finally {
      enabling = false;
    }
  }

  async function openSelectedProfileSource() {
    const sourceUrl = openvinoInstallStore.selectedProfile?.sourceUrl;
    if (sourceUrl) await openUrl(sourceUrl);
  }
</script>

<div class="step">
  <h2>{i18n.m.wizard.steps.openvino.title}</h2>
  <p class="description">{i18n.m.wizard.steps.openvino.description}</p>

  <div class="npu-card">
    <div class="npu-head">
      <div>
        <strong>{settingsStore.npuInfo?.name ?? i18n.m.wizard.steps.openvino.unknownNpu}</strong>
        <span>{settingsStore.npuInfo?.manufacturer ?? i18n.m.wizard.steps.openvino.managedRuntime}</span>
      </div>
      {#if settingsStore.npuInfo?.available}
        <span class="badge ok">{i18n.m.wizard.steps.openvino.detected}</span>
      {:else}
        <span class="badge">{i18n.m.wizard.steps.openvino.optional}</span>
      {/if}
    </div>

    <div class="actions">
      {#if !openvinoInstallStore.status?.installed}
        <button
          class="btn-primary"
          disabled={openvinoInstallStore.installing}
          onclick={() => openvinoInstallStore.install()}
        >
          {openvinoInstallStore.installing ? i18n.m.common.loading : i18n.m.wizard.steps.openvino.install}
        </button>
      {:else}
        <label class="profile-select" for="wizard-openvino-profile">
          <span>{i18n.m.settings.llm.openvinoModelProfile}</span>
          <select
            id="wizard-openvino-profile"
            value={openvinoInstallStore.selectedProfileId}
            onchange={(e) => openvinoInstallStore.setSelectedProfile((e.target as HTMLSelectElement).value)}
          >
            {#each openvinoInstallStore.profiles as profile (profile.id)}
              <option value={profile.id}>{profile.name}</option>
            {/each}
          </select>
        </label>
        {#if openvinoInstallStore.selectedProfile}
          <div class="profile-card">
            <strong>{openvinoInstallStore.selectedProfile.qualityTier}</strong>
            <span>{openvinoInstallStore.selectedProfile.description}</span>
            <small>
              {openvinoInstallStore.selectedProfile.autoDownloadable
                ? i18n.m.settings.llm.openvinoProfileAuto
                : i18n.m.settings.llm.openvinoProfileManual}
            </small>
          </div>
        {/if}
        <button
          class="btn-secondary"
          disabled={openvinoInstallStore.downloadingModel || !openvinoInstallStore.selectedProfile?.autoDownloadable}
          onclick={() => openvinoInstallStore.downloadRecommendedModel()}
        >
          {openvinoInstallStore.downloadingModel ? i18n.m.common.loading : i18n.m.wizard.steps.openvino.downloadModel}
        </button>
        {#if openvinoInstallStore.selectedProfile?.sourceUrl}
          <button class="btn-secondary" onclick={openSelectedProfileSource}>
            {i18n.m.settings.llm.openvinoOpenSource}
          </button>
        {/if}
        <button
          class="btn-primary"
          disabled={enabling || !openvinoInstallStore.modelDir.trim()}
          onclick={enableNpuBackend}
        >
          {enabling ? i18n.m.common.loading : i18n.m.wizard.steps.openvino.useNpu}
        </button>
      {/if}
    </div>

    {#if openvinoInstallStore.currentStep}
      <p class="progress">{openvinoInstallStore.currentStep}</p>
    {/if}
    {#if openvinoInstallStore.error}
      <p class="error">{openvinoInstallStore.error}</p>
    {/if}
  </div>

  <p class="hint">{i18n.m.wizard.steps.openvino.hint}</p>
</div>

<style>
  .step { display: flex; flex-direction: column; gap: 1rem; }
  h2 { font-size: 1.25rem; font-weight: 600; }
  .description,
  .hint {
    color: var(--color-text-muted);
    line-height: 1.7;
    font-size: 0.875rem;
  }

  .npu-card {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 12px;
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding: 1.25rem;
  }

  .npu-head {
    align-items: flex-start;
    display: flex;
    gap: 1rem;
    justify-content: space-between;
  }

  .npu-head div {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    min-width: 0;
  }

  .npu-head strong { font-size: 0.95rem; }
  .npu-head span { color: var(--color-text-muted); font-size: 0.8rem; }

  .badge {
    border: 1px solid var(--color-border);
    border-radius: 999px;
    color: var(--color-text-muted);
    flex-shrink: 0;
    font-size: 0.72rem;
    font-weight: 700;
    padding: 0.2rem 0.55rem;
  }

  .badge.ok {
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 35%, var(--color-border));
    color: var(--color-success);
  }

  .actions { display: flex; flex-wrap: wrap; gap: 0.75rem; }

  .profile-select {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    width: min(100%, 420px);
  }

  .profile-select span {
    color: var(--color-text-muted);
    font-size: 0.78rem;
  }

  .profile-select select {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    color: var(--color-text);
    padding: 0.5rem 0.65rem;
  }

  .profile-card {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    padding: 0.75rem;
  }

  .profile-card span,
  .profile-card small {
    color: var(--color-text-muted);
    font-size: 0.8rem;
    line-height: 1.5;
  }

  .btn-primary,
  .btn-secondary {
    border-radius: 8px;
    cursor: pointer;
    font-size: 0.875rem;
    font-weight: 600;
    padding: 0.5rem 1rem;
  }

  .btn-primary {
    background: var(--color-accent);
    border: none;
    color: #fff;
  }

  .btn-secondary {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    color: var(--color-text);
  }

  button:disabled { cursor: default; opacity: 0.55; }
  .progress { color: var(--color-text-muted); font-size: 0.8rem; margin: 0; }
  .error { color: var(--color-error); font-size: 0.8rem; margin: 0; }
</style>
