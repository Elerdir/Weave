<script lang="ts">
  import { onMount } from "svelte";
  import { themeStore } from "$lib/theme/index.svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import Wizard from "$features/wizard/Wizard.svelte";
  import MainLayout from "$features/chat/MainLayout.svelte";
  import LogWindow from "$features/settings/LogWindow.svelte";

  // Samostatné okno s logy — stejný frontend, jiný „view" (viz open_log_window)
  const isLogWindow =
    new URLSearchParams(window.location.search).get("view") === "logs";

  let ready = $state(false);
  let showWizard = $state(false);

  onMount(async () => {
    // Aplikuj téma ihned při startu
    const resolved = themeStore.resolvedTheme;
    document.documentElement.classList.add(resolved);

    if (isLogWindow) {
      ready = true;
      return;
    }

    // Zkontroluj zda je to první spuštění
    const firstRun = !localStorage.getItem("weave.setup-complete");
    showWizard = firstRun;

    if (!firstRun) {
      // Tauri invoke nemusí být dostupný (např. webové E2E) — chybu spolkneme,
      // ať se UI vždy vykreslí.
      try {
        await conversationStore.loadAll();
      } catch (err) {
        console.warn("loadAll selhal:", err);
      }
    }

    ready = true;
  });

  function onWizardComplete() {
    localStorage.setItem("weave.setup-complete", "1");
    showWizard = false;
    conversationStore.loadAll().catch((err) => console.warn("loadAll selhal:", err));
  }
</script>

{#if ready}
  {#if isLogWindow}
    <LogWindow />
  {:else if showWizard}
    <Wizard onComplete={onWizardComplete} />
  {:else}
    <MainLayout />
  {/if}
{:else}
  <div class="splash">
    <div class="splash-logo">Weave</div>
  </div>
{/if}

<style>
  .splash {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: var(--color-bg);
  }

  .splash-logo {
    font-size: 2rem;
    font-weight: 700;
    color: var(--color-accent);
    letter-spacing: 0.1em;
    opacity: 0.8;
  }
</style>
