<script lang="ts">
  import { i18n } from "$lib/i18n/index.svelte";
  import type { Locale } from "$lib/i18n/index.svelte";

  const locales: { value: Locale; label: string }[] = [
    { value: "cs", label: "Čeština" },
    { value: "en", label: "English" },
  ];
</script>

<div class="step">
  <h2>{i18n.m.wizard.steps.welcome.title}</h2>
  <p class="description">{i18n.m.wizard.steps.welcome.description}</p>

  <div class="field">
    <label for="locale-select">{i18n.m.wizard.steps.welcome.language}</label>
    <select
      id="locale-select"
      value={i18n.locale}
      onchange={(e) => i18n.setLocale((e.target as HTMLSelectElement).value as Locale)}
    >
      {#each locales as loc}
        <option value={loc.value}>{loc.label}</option>
      {/each}
    </select>
  </div>
</div>

<style>
  .step {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  h2 {
    font-size: 1.25rem;
    font-weight: 600;
  }

  .description {
    color: var(--color-text-muted);
    line-height: 1.7;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    margin-top: 0.5rem;
  }

  label {
    font-size: 0.85rem;
    color: var(--color-text-muted);
    font-weight: 500;
  }

  select {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
    font-size: 0.9rem;
    outline: none;
    cursor: pointer;
    width: 200px;
  }

  select:focus {
    border-color: var(--color-accent);
  }
</style>
