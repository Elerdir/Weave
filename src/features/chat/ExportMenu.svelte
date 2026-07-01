<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { save } from "@tauri-apps/plugin-dialog";
  import { conversationStore } from "$lib/stores/conversations.svelte";

  let open = $state(false);
  let busy = $state(false);

  type Format = "markdown" | "html";

  const formats: { id: Format; label: string; ext: string }[] = [
    { id: "markdown", label: "Markdown", ext: "md" },
    { id: "html", label: "HTML", ext: "html" },
  ];

  async function exportAs(format: Format, ext: string) {
    const convId = conversationStore.activeId;
    if (!convId || busy) return;
    open = false;
    busy = true;
    try {
      const suggested = await invoke<string>("suggest_export_filename", {
        conversationId: convId,
        format,
      });
      const path = await save({
        defaultPath: suggested,
        filters: [{ name: format.toUpperCase(), extensions: [ext] }],
      });
      if (path) {
        await invoke("export_conversation", {
          conversationId: convId,
          format,
          outputPath: path,
        });
      }
    } catch (e) {
      console.error("Export selhal:", e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="export-menu">
  <button class="export-btn" onclick={() => (open = !open)} disabled={busy} title="Exportovat konverzaci">
    ⇩
  </button>

  {#if open}
    <button class="backdrop" onclick={() => (open = false)} aria-label="Zavřít"></button>
    <div class="menu" role="menu">
      {#each formats as f}
        <button class="menu-item" onclick={() => exportAs(f.id, f.ext)}>
          {f.label} <span class="ext">.{f.ext}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .export-menu {
    position: relative;
  }

  .export-btn {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
    border-radius: 7px;
    padding: 0.3rem 0.55rem;
    font-size: 0.9rem;
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s;
    line-height: 1;
  }
  .export-btn:hover:not(:disabled) {
    color: var(--color-text);
    border-color: var(--color-text-muted);
  }
  .export-btn:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: transparent;
    border: none;
  }

  .menu {
    position: absolute;
    top: 100%;
    right: 0;
    margin-top: 0.4rem;
    z-index: 25;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 9px;
    box-shadow: 0 6px 22px rgba(0, 0, 0, 0.3);
    padding: 0.35rem;
    min-width: 140px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .menu-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: transparent;
    border: none;
    color: var(--color-text);
    padding: 0.45rem 0.6rem;
    border-radius: 6px;
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
  }
  .menu-item:hover {
    background: var(--color-surface-2);
  }
  .ext {
    color: var(--color-text-muted);
    font-size: 0.75rem;
  }
</style>
