<script lang="ts">
  import { i18n } from "$lib/i18n/index.svelte";
  import LogViewer from "./LogViewer.svelte";
</script>

<div class="log-window">
  <h2>{i18n.m.settings.sections.logs}</h2>
  <LogViewer fill />
</div>

<style>
  .log-window {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    height: 100vh;
    padding: 1rem 1.25rem;
    background: var(--color-bg);
    color: var(--color-text);
    box-sizing: border-box;
  }

  h2 {
    margin: 0;
    font-size: 1.1rem;
  }
</style>
