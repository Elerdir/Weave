<script lang="ts">
  import type { WorkspaceEntry } from "$lib/stores/workspace.svelte";
  import { workspaceStore } from "$lib/stores/workspace.svelte";

  let { entry, depth }: { entry: WorkspaceEntry; depth: number } = $props();

  let expanded = $state(false);
  let children = $state<WorkspaceEntry[]>([]);
  let renaming = $state(false);
  let newName = $state(entry.name);
  let showMenu = $state(false);

  const isDir = $derived(entry.kind === "directory");
  const icon = $derived(
    isDir
      ? expanded ? "📂" : "📁"
      : getFileIcon(entry.name)
  );

  async function toggle() {
    if (!isDir) {
      await workspaceStore.openFileAt(entry.path);
      return;
    }
    expanded = !expanded;
    if (expanded && children.length === 0) {
      children = await workspaceStore.expandDir(entry.path);
    }
  }

  async function commitRename() {
    if (!newName.trim() || newName === entry.name) {
      renaming = false;
      return;
    }
    const parentDir = entry.path.substring(0, entry.path.lastIndexOf(entry.name));
    const newPath = parentDir + newName;
    await workspaceStore.renameEntry(entry.path, newPath);
    renaming = false;
  }

  function getFileIcon(name: string): string {
    const ext = name.split(".").pop()?.toLowerCase() ?? "";
    const map: Record<string, string> = {
      ts: "🟦", js: "🟨", svelte: "🟠", rs: "🦀",
      md: "📝", json: "📋", toml: "⚙️", sql: "🗄️",
      css: "🎨", html: "🌐", png: "🖼️", jpg: "🖼️",
      pdf: "📄", docx: "📘", txt: "📄",
    };
    return map[ext] ?? "📄";
  }
</script>

<div class="node" style="padding-left: {depth * 12 + 8}px">
  <button
    class="node-btn"
    class:active={workspaceStore.openFile?.path === entry.path}
    onclick={toggle}
    oncontextmenu={(e) => { e.preventDefault(); showMenu = !showMenu; }}
  >
    <span class="node-icon">{icon}</span>
    {#if renaming}
      <input
        class="rename-input"
        bind:value={newName}
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => {
          if (e.key === "Enter") commitRename();
          if (e.key === "Escape") renaming = false;
        }}
        onblur={commitRename}
        autofocus
      />
    {:else}
      <span class="node-name">{entry.name}</span>
    {/if}
  </button>

  {#if showMenu}
    <div class="context-menu" role="menu">
      <button onclick={() => { renaming = true; showMenu = false; }}>Přejmenovat</button>
      <button onclick={() => { workspaceStore.deleteEntry(entry.path); showMenu = false; }}>Smazat</button>
      {#if isDir}
        <button onclick={() => {
          workspaceStore.createEntry(entry.path + "/nový_soubor.txt", false);
          showMenu = false;
        }}>Nový soubor</button>
        <button onclick={() => {
          workspaceStore.createEntry(entry.path + "/nová_složka", true);
          showMenu = false;
        }}>Nová složka</button>
      {/if}
    </div>
  {/if}
</div>

{#if isDir && expanded}
  {#each children as child (child.path)}
    <svelte:self entry={child} depth={depth + 1} />
  {/each}
{/if}

<style>
  .node {
    position: relative;
  }

  .node-btn {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    background: transparent;
    border: none;
    color: var(--color-text);
    cursor: pointer;
    padding: 0.28rem 0.5rem 0.28rem 0;
    border-radius: 5px;
    font-size: 0.85rem;
    text-align: left;
    transition: background 0.1s;
  }

  .node-btn:hover { background: var(--color-surface-2); }
  .node-btn.active { background: var(--color-user-bubble); }

  .node-icon { font-size: 0.9rem; flex-shrink: 0; }

  .node-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rename-input {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-accent);
    border-radius: 4px;
    padding: 0 0.25rem;
    font-size: 0.85rem;
    width: 100%;
    outline: none;
  }

  .context-menu {
    position: absolute;
    left: 100%;
    top: 0;
    z-index: 50;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.35rem;
    display: flex;
    flex-direction: column;
    min-width: 140px;
    box-shadow: 0 4px 16px rgba(0,0,0,0.3);
  }

  .context-menu button {
    background: transparent;
    border: none;
    color: var(--color-text);
    cursor: pointer;
    padding: 0.35rem 0.6rem;
    border-radius: 5px;
    font-size: 0.82rem;
    text-align: left;
    transition: background 0.1s;
  }

  .context-menu button:hover { background: var(--color-surface-2); }
</style>
