<script lang="ts">
  import { i18n } from "$lib/i18n/index.svelte";
  import WizardWelcome from "./steps/WizardWelcome.svelte";
  import WizardApiKeys from "./steps/WizardApiKeys.svelte";
  import WizardGpu from "./steps/WizardGpu.svelte";
  import WizardModels from "./steps/WizardModels.svelte";

  let { onComplete }: { onComplete: () => void } = $props();

  const STEPS = 4;
  let step = $state(1);

  function next() {
    if (step < STEPS) step++;
    else onComplete();
  }

  function back() {
    if (step > 1) step--;
  }
</script>

<div class="wizard-overlay">
  <div class="wizard-card">
    <header class="wizard-header">
      <h1>{i18n.t("app.name")}</h1>
      <span class="step-indicator">
        {i18n.t("wizard.step", { current: step, total: STEPS })}
      </span>
    </header>

    <div class="wizard-progress">
      {#each Array(STEPS) as _, i}
        <div class="progress-dot" class:active={i + 1 === step} class:done={i + 1 < step}></div>
      {/each}
    </div>

    <div class="wizard-body">
      {#if step === 1}
        <WizardWelcome />
      {:else if step === 2}
        <WizardApiKeys />
      {:else if step === 3}
        <WizardGpu />
      {:else}
        <WizardModels />
      {/if}
    </div>

    <footer class="wizard-footer">
      {#if step > 1}
        <button class="btn-ghost" onclick={back}>{i18n.m.wizard.back}</button>
      {:else}
        <div></div>
      {/if}
      <button class="btn-primary" onclick={next}>
        {step === STEPS ? i18n.m.wizard.finish : i18n.m.wizard.next}
      </button>
    </footer>
  </div>
</div>

<style>
  .wizard-overlay {
    position: fixed;
    inset: 0;
    background: var(--color-bg);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .wizard-card {
    width: 560px;
    max-width: calc(100vw - 2rem);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 16px;
    display: flex;
    flex-direction: column;
    gap: 0;
    overflow: hidden;
  }

  .wizard-header {
    padding: 2rem 2rem 1rem;
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .wizard-header h1 {
    font-size: 1.5rem;
    font-weight: 700;
    color: var(--color-accent);
  }

  .step-indicator {
    font-size: 0.8rem;
    color: var(--color-text-muted);
  }

  .wizard-progress {
    display: flex;
    gap: 0.5rem;
    padding: 0 2rem;
  }

  .progress-dot {
    height: 4px;
    flex: 1;
    background: var(--color-border);
    border-radius: 2px;
    transition: background 0.3s;
  }

  .progress-dot.active {
    background: var(--color-accent);
  }

  .progress-dot.done {
    background: var(--color-accent);
    opacity: 0.4;
  }

  .wizard-body {
    padding: 2rem;
    min-height: 280px;
  }

  .wizard-footer {
    padding: 1.5rem 2rem;
    border-top: 1px solid var(--color-border);
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .btn-primary {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 8px;
    padding: 0.6rem 1.5rem;
    font-size: 0.9rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  .btn-primary:hover {
    background: var(--color-accent-hover);
  }

  .btn-ghost {
    background: transparent;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.6rem 1.5rem;
    font-size: 0.9rem;
    cursor: pointer;
    transition: color 0.2s, border-color 0.2s;
  }

  .btn-ghost:hover {
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }
</style>
