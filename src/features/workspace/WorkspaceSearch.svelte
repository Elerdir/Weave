<script lang="ts">
  import { workspaceStore } from "$lib/stores/workspace.svelte";

  let query = $state("");
  let debounce: ReturnType<typeof setTimeout>;

  function onInput() {
    clearTimeout(debounce);
    debounce = setTimeout(() => workspaceStore.search(query), 300);
  }

  function formatSnippet(text: string, query: string): string {
    const idx = text.toLowerCase().indexOf(query.toLowerCase());
    if (idx === -1) return text.slice(0, 120) + "…";
    const start = Math.max(0, idx - 40);
    const end = Math.min(text.length, idx + query.length + 80);
    const before = start > 0 ? "…" : "";
    const after = end < text.length ? "…" : "";
    const snippet = text.slice(start, end);
    return before + snippet.replace(
      new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi"),
      (m) => `<mark>${m}</mark>`
    ) + after;
  }
</script>

<div class="ws-search">
  <div class="search-bar">
    <input
      type="search"
      placeholder="Hledat v souborech..."
      bind:value={query}
      oninput={onInput}
      autofocus
    />
  </div>

  <div class="results">
    {#each workspaceStore.searchResults as result (result.path)}
      <button
        class="result-item"
        onclick={() => workspaceStore.openFileAt(result.path)}
      >
        <div class="result-name">{result.name}</div>
        <div class="result-path">{result.path}</div>
        {#if result.text_content && query}
          <!-- eslint-disable-next-line svelte/no-at-html-tags -->
          <div class="result-snippet">{@html formatSnippet(result.text_content, query)}</div>
        {/if}
      </button>
    {/each}

    {#if query && workspaceStore.searchResults.length === 0}
      <div class="no-results">Nic nenalezeno pro „{query}"</div>
    {/if}
  </div>
</div>

<style>
  .ws-search {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .search-bar {
    padding: 0.6rem 0.75rem;
    border-bottom: 1px solid var(--color-border);
  }

  input {
    width: 100%;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 7px;
    padding: 0.4rem 0.65rem;
    font-size: 0.85rem;
    outline: none;
  }

  input:focus { border-color: var(--color-accent); }

  .results {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0.4rem;
  }

  .result-item {
    background: transparent;
    border: none;
    border-radius: 7px;
    padding: 0.6rem 0.75rem;
    text-align: left;
    cursor: pointer;
    transition: background 0.1s;
    width: 100%;
  }

  .result-item:hover { background: var(--color-surface-2); }

  .result-name {
    font-weight: 600;
    font-size: 0.85rem;
    color: var(--color-text);
  }

  .result-path {
    font-size: 0.75rem;
    color: var(--color-text-muted);
    margin: 0.1rem 0 0.35rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .result-snippet {
    font-size: 0.78rem;
    color: var(--color-text-muted);
    line-height: 1.5;
    overflow: hidden;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .result-snippet :global(mark) {
    background: var(--color-accent);
    color: #fff;
    border-radius: 2px;
    padding: 0 1px;
  }

  .no-results {
    padding: 1rem;
    font-size: 0.82rem;
    color: var(--color-text-muted);
    text-align: center;
  }
</style>
