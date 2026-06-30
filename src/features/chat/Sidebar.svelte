<script lang="ts">
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { i18n } from "$lib/i18n/index.svelte";

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
</script>

<aside class="sidebar">
  <div class="sidebar-header">
    <span class="logo">Weave</span>
    <button class="btn-new" onclick={newConversation} title={i18n.m.chat.newConversation}>
      +
    </button>
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
      <button
        class="conv-item"
        class:active={conv.id === conversationStore.activeId}
        class:pinned={conv.pinned}
        onclick={() => conversationStore.select(conv.id)}
      >
        {#if conv.pinned}<span class="pin">📌</span>{/if}
        <span class="conv-title">{conv.title}</span>
      </button>
    {/each}

    {#if filtered.length === 0}
      <div class="empty-list">
        {search ? "Nic nenalezeno" : "Žádné konverzace"}
      </div>
    {/if}
  </nav>
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

  .conv-item {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    width: 100%;
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
</style>
