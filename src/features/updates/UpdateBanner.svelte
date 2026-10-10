<script lang="ts">
  import { i18n } from "$lib/i18n/index.svelte";
  import { updaterStore } from "$lib/stores/updater.svelte";

  let showNotes = $state(false);
</script>

{#if updaterStore.bannerVisible}
  <div class="update-banner" role="status" aria-live="polite" data-testid="update-banner">
    {#if updaterStore.phase === "downloading"}
      <div class="row">
        <span class="title">{i18n.m.settings.updates.downloading}</span>
        <span class="muted">{updaterStore.percent}%</span>
      </div>
      <div class="bar"><div class="fill" style="width: {updaterStore.percent}%"></div></div>
    {:else if updaterStore.phase === "readyToRestart"}
      <span class="title">{i18n.m.settings.updates.readyToRestart}</span>
      <div class="actions">
        <button class="primary" onclick={() => updaterStore.restart()}>
          {i18n.m.settings.updates.restartNow}
        </button>
      </div>
    {:else if updaterStore.phase === "error"}
      <span class="title">{i18n.m.updates.banner.failed}</span>
      <span class="muted error">{updaterStore.error}</span>
      <div class="actions">
        <button onclick={() => updaterStore.dismiss()}>{i18n.m.updates.banner.later}</button>
        <button class="primary" onclick={() => updaterStore.downloadAndInstall()}>
          {i18n.m.updates.banner.retry}
        </button>
      </div>
    {:else}
      <span class="title">
        {i18n.t("updates.banner.available", { version: updaterStore.version ?? "" })}
      </span>
      {#if updaterStore.notes}
        <button class="link" onclick={() => (showNotes = !showNotes)}>
          {showNotes ? i18n.m.updates.banner.hideNotes : i18n.m.updates.banner.showNotes}
        </button>
        {#if showNotes}
          <pre class="notes">{updaterStore.notes}</pre>
        {/if}
      {/if}
      <div class="actions">
        <button onclick={() => updaterStore.dismiss()}>{i18n.m.updates.banner.later}</button>
        <button class="primary" onclick={() => updaterStore.downloadAndInstall()}>
          {i18n.m.settings.updates.downloadInstall}
        </button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .update-banner {
    position: fixed;
    right: 1rem;
    bottom: 1rem;
    z-index: 1000;
    width: min(22rem, calc(100vw - 2rem));
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.875rem 1rem;
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-left: 3px solid var(--color-accent);
    border-radius: 8px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
    font-size: 0.875rem;
  }

  .row {
    display: flex;
    justify-content: space-between;
  }

  .title {
    font-weight: 600;
  }

  .muted {
    color: var(--color-text-muted);
  }

  .error {
    color: var(--color-error);
    word-break: break-word;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
  }

  button {
    padding: 0.35rem 0.75rem;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text);
    cursor: pointer;
  }

  button.primary {
    background: var(--color-accent);
    border-color: var(--color-accent);
    color: white;
  }

  button.primary:hover {
    background: var(--color-accent-hover);
  }

  button.link {
    align-self: flex-start;
    padding: 0;
    border: none;
    color: var(--color-accent);
  }

  .notes {
    max-height: 10rem;
    overflow: auto;
    margin: 0;
    white-space: pre-wrap;
    font-family: inherit;
    color: var(--color-text-muted);
  }

  .bar {
    height: 6px;
    border-radius: 3px;
    background: var(--color-border);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--color-accent);
    transition: width 0.2s;
  }
</style>
