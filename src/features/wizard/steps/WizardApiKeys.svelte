<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open as openUrl } from "@tauri-apps/plugin-shell";
  import { i18n } from "$lib/i18n/index.svelte";
  import { TOKEN_URLS } from "$lib/token-urls";
  import type { ApiServiceId as Service } from "$lib/token-urls";

  interface KeyField {
    service: Service;
    label: string;
    value: string;
    saving: boolean;
    saved: boolean;
    error: string | null;
  }

  let fields = $state<KeyField[]>([
    { service: "mistral", label: "", value: "", saving: false, saved: false, error: null },
    { service: "civitai", label: "", value: "", saving: false, saved: false, error: null },
    { service: "huggingface", label: "", value: "", saving: false, saved: false, error: null },
  ]);

  onMount(async () => {
    // Krok jde ve wizardu navštívit opakovaně (zpět/vpřed) — bez tohohle by
    // se checkmark po každém návratu vynuloval, i kdyby byl klíč reálně uložený.
    await Promise.all(
      fields.map(async (field) => {
        try {
          field.saved = await invoke<boolean>("get_api_key_status", { service: field.service });
        } catch (e) {
          console.warn(`Nepodařilo se zjistit stav klíče ${field.service}:`, e);
        }
      }),
    );
  });

  async function openTokenPage(service: Service) {
    try {
      await openUrl(TOKEN_URLS[service]);
    } catch (e) {
      console.warn("Nepodařilo se otevřít odkaz:", e);
    }
  }

  $effect(() => {
    fields[0].label = i18n.m.wizard.steps.apiKeys.mistral;
    fields[1].label = i18n.m.wizard.steps.apiKeys.civitai;
    fields[2].label = i18n.m.wizard.steps.apiKeys.huggingface;
  });

  async function save(field: KeyField) {
    if (!field.value.trim()) return;
    field.saving = true;
    field.error = null;
    try {
      await invoke("store_api_key", { service: field.service, token: field.value.trim() });
      field.saved = true;
      field.value = "";
    } catch (e) {
      // Dřív se chyba jen tiše zahodila (unhandled rejection) — uživatel
      // neměl žádnou zpětnou vazbu, jestli se klíč vůbec uložil.
      field.error = String(e);
    } finally {
      field.saving = false;
    }
  }
</script>

<div class="step">
  <h2>{i18n.m.wizard.steps.apiKeys.title}</h2>
  <p class="description">{i18n.m.wizard.steps.apiKeys.description}</p>

  <div class="fields">
    {#each fields as field}
      <div class="field">
        <div class="label-row">
          <label for={field.service}>{field.label}</label>
          <button
            type="button"
            class="link-btn"
            onclick={() => openTokenPage(field.service)}
            title={i18n.m.wizard.steps.apiKeys.howToGet}
            aria-label={i18n.m.wizard.steps.apiKeys.howToGet}
          >🌐</button>
        </div>
        <div class="input-row">
          <input
            id={field.service}
            type="password"
            placeholder={i18n.m.wizard.steps.apiKeys.placeholder}
            bind:value={field.value}
            onkeydown={(e) => e.key === "Enter" && save(field)}
            disabled={field.saving}
          />
          <button
            class="btn-save"
            onclick={() => save(field)}
            disabled={!field.value.trim() || field.saving}
          >
            {#if field.saved}
              ✓
            {:else if field.saving}
              ...
            {:else}
              {i18n.m.common.save}
            {/if}
          </button>
        </div>
        {#if field.error}
          <span class="error-hint">{i18n.m.wizard.steps.apiKeys.saveFailed}: {field.error}</span>
        {:else if field.saved}
          <span class="saved-hint">{i18n.m.settings.apiKeys.stored}</span>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .step {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  h2 { font-size: 1.25rem; font-weight: 600; }

  .description {
    color: var(--color-text-muted);
    line-height: 1.7;
    font-size: 0.875rem;
  }

  .fields {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    margin-top: 0.5rem;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
  }

  .label-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  label {
    font-size: 0.82rem;
    font-weight: 500;
    color: var(--color-text-muted);
  }

  .link-btn {
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 0.9rem;
    padding: 0.1rem 0.3rem;
    border-radius: 5px;
    opacity: 0.7;
    transition: opacity 0.15s, background 0.15s;
  }
  .link-btn:hover {
    opacity: 1;
    background: var(--color-surface-2);
  }

  .input-row {
    display: flex;
    gap: 0.5rem;
  }

  input {
    flex: 1;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
    font-size: 0.9rem;
    outline: none;
    font-family: monospace;
  }

  input:focus {
    border-color: var(--color-accent);
  }

  .btn-save {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 8px;
    padding: 0.5rem 1rem;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
    min-width: 60px;
    transition: background 0.2s;
  }

  .btn-save:hover:not(:disabled) { background: var(--color-accent-hover); }
  .btn-save:disabled { opacity: 0.5; cursor: default; }

  .saved-hint {
    font-size: 0.78rem;
    color: var(--color-success);
  }

  .error-hint {
    font-size: 0.78rem;
    color: var(--color-error);
  }
</style>
