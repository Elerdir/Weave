<script lang="ts">
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { i18n } from "$lib/i18n/index.svelte";

  let {
    showWorkspace = $bindable(false),
    onOpenSettings,
  }: { showWorkspace?: boolean; onOpenSettings?: () => void } = $props();

  let search = $state("");

  const filtered = $derived(
    search.trim()
      ? conversationStore.conversations.filter(c =>
          c.title.toLowerCase().includes(search.toLowerCase())
        )
      : conversationStore.conversations
  );

  async function newConversation() {
    const title = i18n.m.chat.newConversation;
    await conversationStore.create(`${title} ${conversationStore.conversations.length + 1}`);
  }

  // Context menu / inline rename stav
  let menuFor = $state<string | null>(null);
  let renamingId = $state<string | null>(null);
  let renameValue = $state("");

  function openMenu(e: MouseEvent, id: string) {
    e.preventDefault();
    menuFor = menuFor === id ? null : id;
  }

  function startRename(id: string, current: string) {
    renamingId = id;
    renameValue = current;
    menuFor = null;
  }

  async function commitRename(id: string) {
    if (renameValue.trim()) {
      await conversationStore.rename(id, renameValue);
    }
    renamingId = null;
  }

  async function confirmDelete(id: string) {
    menuFor = null;
    if (confirm(i18n.m.sidebar.deleteConfirm)) {
      await conversationStore.delete(id);
    }
  }
</script>

<aside class="sidebar">
  <div class="sidebar-header">
    <span class="logo">Weave</span>
    <div class="header-btns">
      <button
        class="btn-icon"
        class:active={showWorkspace}
        onclick={() => showWorkspace = !showWorkspace}
        title="Workspace"
      >📁</button>
      <button class="btn-new" onclick={newConversation} title={i18n.m.chat.newConversation}>
        +
      </button>
    </div>
  </div>

  <div class="search-wrap">
    <input
      class="search"
      type="search"
      placeholder={i18n.m.sidebar.search}
      bind:value={search}
    />
  </div>

  <nav class="conv-list">
    {#each filtered as conv (conv.id)}
      <div class="conv-row">
        {#if renamingId === conv.id}
          <input
            class="rename-input"
            bind:value={renameValue}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename(conv.id);
              if (e.key === "Escape") renamingId = null;
            }}
            onblur={() => commitRename(conv.id)}
            autofocus
          />
        {:else}
          <button
            class="conv-item"
            class:active={conv.id === conversationStore.activeId}
            class:pinned={conv.pinned}
            onclick={() => conversationStore.select(conv.id)}
            oncontextmenu={(e) => openMenu(e, conv.id)}
          >
            {#if conv.pinned}<span class="pin">📌</span>{/if}
            <span class="conv-title">{conv.title}</span>
          </button>
          <button
            class="conv-menu-btn"
            onclick={(e) => openMenu(e, conv.id)}
            aria-label="Možnosti"
          >⋯</button>
        {/if}

        {#if menuFor === conv.id}
          <button class="backdrop" onclick={() => (menuFor = null)} aria-label="Zavřít"></button>
          <div class="conv-menu" role="menu">
            <button onclick={() => startRename(conv.id, conv.title)}>
              {i18n.m.sidebar.rename}
            </button>
            <button onclick={() => conversationStore.togglePin(conv.id).then(() => (menuFor = null))}>
              {conv.pinned ? i18n.m.sidebar.unpin : i18n.m.sidebar.pin}
            </button>
            <button class="danger" onclick={() => confirmDelete(conv.id)}>
              {i18n.m.sidebar.delete}
            </button>
          </div>
        {/if}
      </div>
    {/each}

    {#if filtered.length === 0}
      <div class="empty-list">
        {search ? "Nic nenalezeno" : "Žádné konverzace"}
      </div>
    {/if}
  </nav>

  <footer class="sidebar-footer">
    <button class="footer-btn" onclick={() => onOpenSettings?.()} title={i18n.m.settings.title}>
      ⚙ {i18n.m.sidebar.settings}
    </button>
  </footer>
</aside>

<style>
  .sidebar {
    width: 260px;
    min-width: 200px;
    max-width: 320px;
    background: var(--color-surface);
    border-right: 1px solid var(--color-border);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1rem 1rem 0.75rem;
  }

  .logo {
    font-weight: 700;
    font-size: 1.1rem;
    color: var(--color-accent);
    letter-spacing: 0.05em;
  }

  .header-btns {
    display: flex;
    gap: 4px;
    align-items: center;
  }

  .btn-icon {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: 0.2rem 0.3rem;
    border-radius: 5px;
    font-size: 0.9rem;
    transition: color 0.15s, background 0.15s;
    line-height: 1;
  }

  .btn-icon:hover { color: var(--color-text); background: var(--color-surface-2); }
  .btn-icon.active { color: var(--color-accent); }

  .btn-new {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 1.2rem;
    cursor: pointer;
    transition: background 0.2s;
    line-height: 1;
  }

  .btn-new:hover { background: var(--color-border); }

  .search-wrap {
    padding: 0 0.75rem 0.75rem;
  }

  .search {
    width: 100%;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.45rem 0.75rem;
    font-size: 0.85rem;
    outline: none;
  }

  .search:focus { border-color: var(--color-accent); }

  .conv-list {
    flex: 1;
    overflow-y: auto;
    padding: 0 0.5rem 1rem;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .conv-row {
    position: relative;
    display: flex;
    align-items: center;
  }

  .conv-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    flex: 1;
    min-width: 0;
    text-align: left;
    background: transparent;
    color: var(--color-text);
    border: none;
    border-radius: 8px;
    padding: 0.55rem 0.75rem;
    font-size: 0.875rem;
    cursor: pointer;
    transition: background 0.15s;
    white-space: nowrap;
    overflow: hidden;
  }

  .conv-item:hover { background: var(--color-surface-2); }
  .conv-item.active { background: var(--color-user-bubble); color: var(--color-text); }

  .conv-menu-btn {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: 0.2rem 0.35rem;
    border-radius: 5px;
    font-size: 1rem;
    line-height: 1;
    opacity: 0;
    transition: opacity 0.15s, color 0.15s;
  }
  .conv-row:hover .conv-menu-btn { opacity: 1; }
  .conv-menu-btn:hover { color: var(--color-text); background: var(--color-surface-2); }

  .rename-input {
    flex: 1;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-accent);
    border-radius: 8px;
    padding: 0.5rem 0.7rem;
    font-size: 0.875rem;
    outline: none;
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: transparent;
    border: none;
  }

  .conv-menu {
    position: absolute;
    right: 0.25rem;
    top: 100%;
    z-index: 25;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    box-shadow: 0 6px 20px rgba(0, 0, 0, 0.3);
    padding: 0.3rem;
    display: flex;
    flex-direction: column;
    min-width: 130px;
  }
  .conv-menu button {
    background: transparent;
    border: none;
    color: var(--color-text);
    text-align: left;
    padding: 0.4rem 0.6rem;
    border-radius: 6px;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .conv-menu button:hover { background: var(--color-surface-2); }
  .conv-menu button.danger { color: var(--color-error); }

  .conv-title {
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
  }

  .pin { font-size: 0.75rem; }

  .empty-list {
    padding: 1rem 0.75rem;
    font-size: 0.82rem;
    color: var(--color-text-muted);
    text-align: center;
  }

  .sidebar-footer {
    border-top: 1px solid var(--color-border);
    padding: 0.5rem;
  }

  .footer-btn {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    padding: 0.55rem 0.75rem;
    border-radius: 8px;
    font-size: 0.875rem;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }

  .footer-btn:hover {
    background: var(--color-surface-2);
    color: var(--color-text);
  }
</style>
