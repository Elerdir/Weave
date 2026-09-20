<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { themeStore } from "$lib/theme/index.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import Wizard from "$features/wizard/Wizard.svelte";
  import MainLayout from "$features/chat/MainLayout.svelte";
  import LogWindow from "$features/settings/LogWindow.svelte";
  import Settings from "$features/settings/Settings.svelte";
  import GalleryWindow from "$features/gallery/GalleryWindow.svelte";
  import GalleryDetailWindow from "$features/gallery/GalleryDetailWindow.svelte";
  import SubjectsWindow from "$features/subjects/SubjectsWindow.svelte";

  // Samostatná okna — stejný frontend, jiný „view" (viz open_*_window)
  const view = new URLSearchParams(window.location.search).get("view");
  const isLogWindow = view === "logs";
  const isSettingsWindow = view === "settings";
  const isGalleryWindow = view === "gallery";
  const isGalleryDetailWindow = view === "gallery-detail";
  const isSubjectsWindow = view === "subjects";

  let ready = $state(false);
  let showWizard = $state(false);

  onMount(async () => {
    // Aplikuj téma ihned při startu
    const resolved = themeStore.resolvedTheme;
    document.documentElement.classList.add(resolved);

    // Motiv a jazyk žijí v databázi; dokud se nenačtou, UI se nevykresluje
    // (`ready`), takže nic neproblikne výchozím nastavením. Každé okno si je
    // načítá samo — settings i galerie jsou samostatné webview.
    await Promise.all([themeStore.hydrate(), i18n.hydrate()]);

    if (isLogWindow || isSettingsWindow || isGalleryWindow || isGalleryDetailWindow || isSubjectsWindow) {
      ready = true;
      return;
    }

    const firstRun = await isFirstRun();
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

  /**
   * Má se ukázat úvodní průvodce? Rozhoduje backend podle databáze — dřív to
   * bylo v `localStorage`, jenže ten žije v datové složce WebView2 a při
   * poškození její leveldb se zápisy tiše zahazují, takže průvodce naskakoval
   * po každém spuštění. Fallback na `localStorage` zůstává pro běh mimo Tauri
   * (webové e2e testy), kde `invoke` neexistuje.
   */
  async function isFirstRun(): Promise<boolean> {
    try {
      return await invoke<boolean>("needs_setup");
    } catch (err) {
      console.warn("needs_setup selhal, používám localStorage:", err);
      return !localStorage.getItem("weave.setup-complete");
    }
  }

  function onWizardComplete() {
    localStorage.setItem("weave.setup-complete", "1");
    invoke("mark_setup_complete").catch((err) =>
      console.warn("mark_setup_complete selhal:", err)
    );
    showWizard = false;
    conversationStore.loadAll().catch((err) => console.warn("loadAll selhal:", err));
  }
</script>

{#if ready}
  {#if isLogWindow}
    <LogWindow />
  {:else if isSettingsWindow}
    <Settings onClose={() => window.close()} windowMode />
  {:else if isGalleryWindow}
    <GalleryWindow />
  {:else if isGalleryDetailWindow}
    <GalleryDetailWindow />
  {:else if isSubjectsWindow}
    <SubjectsWindow />
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
