<script lang="ts">
  import { onMount } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { workspaceStore } from "$lib/stores/workspace.svelte";
  import FileTree from "./FileTree.svelte";
  import FileEditor from "./FileEditor.svelte";
  import WorkspaceSearch from "./WorkspaceSearch.svelte";

  let view = $state<"tree" | "search">("tree");

  onMount(() => workspaceStore.load());

  async function pickFolder() {
    const selected = await open({ directory: true, multiple: false, title: "Vybrat workspace složku" });
    if (typeof selected === "string") {
      await workspaceStore.setRoot(selected);
    }
  }
</script>

<div class="workspace-panel">
  <header class="ws-header">
    <span class="ws-title">
      {workspaceStore.root
        ? workspaceStore.root.split(/[\\/]/).at(-1)
        : "Workspace"}
    </span>
    <div class="ws-actions">
      <button
        class="icon-btn"
        class:active={view === "search"}
        onclick={() => view = view === "search" ? "tree" : "search"}
        title="Hledat v souborech"
      >🔍</button>
      <button class="icon-btn" onclick={pickFolder} title="Otevřít složku">📂</button>
      {#if workspaceStore.root}
        <button
          class="icon-btn"
          onclick={() => workspaceStore.startIndex()}
          disabled={workspaceStore.indexing}
          title="Indexovat"
        >
          {workspaceStore.indexing ? "⏳" : "⟳"}
        </button>
      {/if}
    </div>
  </header>

  {#if workspaceStore.indexing && workspaceStore.indexProgress}
    <div class="index-bar">
      {#if workspaceStore.indexProgress.type === "file"}
        <div class="index-progress">
          <div
            class="index-fill"
            style="width: {Math.round(((workspaceStore.indexProgress.indexed ?? 0) / (workspaceStore.indexProgress.total ?? 1)) * 100)}%"
          ></div>
        </div>
        <span class="index-label">
          {workspaceStore.indexProgress.indexed} / {workspaceStore.indexProgress.total}
        </span>
      {:else if workspaceStore.indexProgress.type === "done"}
        <span class="index-label done">Indexováno ✓</span>
      {/if}
    </div>
  {/if}

  <div class="ws-body">
    {#if !workspaceStore.root}
      <div class="empty-ws">
        <p>Žádný workspace</p>
        <button class="btn-open" onclick={pickFolder}>Otevřít složku</button>
      </div>
    {:else if view === "search"}
      <WorkspaceSearch />
    {:else}
      <FileTree entries={workspaceStore.tree} root={workspaceStore.root} />
    {/if}
  </div>

  {#if workspaceStore.openFile}
    <FileEditor />
  {/if}
</div>

<style>
  .workspace-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--color-surface);
    border-right: 1px solid var(--color-border);
  }

  .ws-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--color-border);
    min-height: 40px;
  }

  .ws-title {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 140px;
  }

  .ws-actions {
    display: flex;
    gap: 2px;
  }

  .icon-btn {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: 0.2rem 0.35rem;
    border-radius: 4px;
    font-size: 0.85rem;
    transition: color 0.15s, background 0.15s;
    line-height: 1;
  }

  .icon-btn:hover:not(:disabled) {
    color: var(--color-text);
    background: var(--color-surface-2);
  }

  .icon-btn.active { color: var(--color-accent); }
  .icon-btn:disabled { opacity: 0.4; cursor: default; }

  .index-bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.35rem 0.75rem;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface-2);
  }

  .index-progress {
    flex: 1;
    height: 4px;
    background: var(--color-border);
    border-radius: 2px;
    overflow: hidden;
  }

  .index-fill {
    height: 100%;
    background: var(--color-accent);
    border-radius: 2px;
    transition: width 0.2s;
  }

  .index-label {
    font-size: 0.75rem;
    color: var(--color-text-muted);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .index-label.done { color: var(--color-success); }

  .ws-body {
    flex: 1;
    overflow-y: auto;
  }

  .empty-ws {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 0.75rem;
    padding: 1rem;
    color: var(--color-text-muted);
    font-size: 0.85rem;
    text-align: center;
  }

  .btn-open {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 8px;
    padding: 0.5rem 1rem;
    font-size: 0.85rem;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.2s;
  }

  .btn-open:hover { background: var(--color-accent-hover); }
</style>
