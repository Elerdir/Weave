<script lang="ts">
  import { workspaceStore } from "$lib/stores/workspace.svelte";

  const file = $derived(workspaceStore.openFile);
  const fileName = $derived(file?.path.split(/[\\/]/).at(-1) ?? "");
  const ext = $derived(fileName.split(".").pop()?.toLowerCase() ?? "");

  function onInput(e: Event) {
    workspaceStore.updateOpenContent((e.target as HTMLTextAreaElement).value);
  }

  async function onKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      await workspaceStore.saveOpenFile();
    }
    if (e.key === "Escape") {
      workspaceStore.closeFile();
    }
  }
</script>

{#if file}
  <div class="editor-overlay">
    <div class="editor-header">
      <span class="file-name">{fileName}</span>
      {#if file.dirty}
        <span class="dirty-dot" title="Neuloženo">●</span>
      {/if}
      <div class="editor-actions">
        <button class="btn-save" onclick={() => workspaceStore.saveOpenFile()} disabled={!file.dirty}>
          Uložit
        </button>
        <button class="btn-close" onclick={() => workspaceStore.closeFile()}>✕</button>
      </div>
    </div>

    <textarea
      class="editor-area"
      class:monospace={["ts","js","rs","py","json","toml","sql","sh","css","html","svelte"].includes(ext)}
      value={file.content}
      oninput={onInput}
      onkeydown={onKeydown}
      spellcheck={false}
      autocomplete="off"
      autocapitalize="off"
    ></textarea>

    <div class="editor-footer">
      <span>{ext.toUpperCase() || "TXT"}</span>
      <span>{file.content.split("\n").length} řádků</span>
      <span>{file.content.length} znaků</span>
      <span class="shortcut">Ctrl+S uložit · Esc zavřít</span>
    </div>
  </div>
{/if}

<style>
  .editor-overlay {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    flex-direction: column;
    background: var(--color-bg);
  }

  .editor-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.6rem 1rem;
    background: var(--color-surface);
    border-bottom: 1px solid var(--color-border);
  }

  .file-name {
    font-weight: 600;
    font-size: 0.9rem;
  }

  .dirty-dot {
    color: var(--color-warning);
    font-size: 0.75rem;
  }

  .editor-actions {
    display: flex;
    gap: 0.5rem;
    margin-left: auto;
  }

  .btn-save {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 6px;
    padding: 0.3rem 0.75rem;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  .btn-save:hover:not(:disabled) { background: var(--color-accent-hover); }
  .btn-save:disabled { opacity: 0.4; cursor: default; }

  .btn-close {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
    border-radius: 6px;
    padding: 0.3rem 0.6rem;
    font-size: 0.8rem;
    cursor: pointer;
  }

  .btn-close:hover { color: var(--color-text); }

  .editor-area {
    flex: 1;
    background: var(--color-bg);
    color: var(--color-text);
    border: none;
    outline: none;
    padding: 1.25rem 1.5rem;
    font-size: 0.9rem;
    line-height: 1.7;
    resize: none;
    tab-size: 2;
  }

  .editor-area.monospace {
    font-family: "JetBrains Mono", "Fira Code", "Cascadia Code", monospace;
    font-size: 0.85rem;
  }

  .editor-footer {
    display: flex;
    gap: 1.5rem;
    padding: 0.3rem 1rem;
    background: var(--color-surface);
    border-top: 1px solid var(--color-border);
    font-size: 0.75rem;
    color: var(--color-text-muted);
  }

  .shortcut { margin-left: auto; }
</style>
