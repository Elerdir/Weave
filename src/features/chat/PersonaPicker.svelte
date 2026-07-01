<script lang="ts">
  import { onMount } from "svelte";
  import { personaStore } from "$lib/stores/personas.svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";

  let open = $state(false);
  let creating = $state(false);
  let newName = $state("");
  let newIcon = $state("🎭");
  let newPrompt = $state("");

  const currentId = $derived(conversationStore.activeConversation?.persona_id ?? null);
  const current = $derived(personaStore.byId(currentId));

  onMount(() => {
    personaStore.load().catch((e) => console.warn("persona load selhal:", e));
  });

  async function pick(id: string | null) {
    await conversationStore.setActivePersona(id);
    open = false;
  }

  async function submitNew() {
    if (!newName.trim() || !newPrompt.trim()) return;
    const p = await personaStore.create(newName, newIcon, newPrompt);
    newName = "";
    newIcon = "🎭";
    newPrompt = "";
    creating = false;
    await pick(p.id);
  }
</script>

<div class="persona-picker">
  <button class="persona-btn" onclick={() => (open = !open)}>
    {#if current}
      <span>{current.icon}</span>
      <span class="pname">{current.name}</span>
    {:else}
      <span>🎭</span>
      <span class="pname muted">Bez persony</span>
    {/if}
    <span class="caret">▾</span>
  </button>

  {#if open}
    <button class="backdrop" onclick={() => (open = false)} aria-label="Zavřít"></button>
    <div class="persona-menu" role="listbox">
      <button class="menu-item" class:active={currentId === null} onclick={() => pick(null)}>
        <span>🎭</span> <span class="mi-name muted">Bez persony</span>
      </button>

      {#each personaStore.personas as p (p.id)}
        <div class="menu-row">
          <button class="menu-item" class:active={currentId === p.id} onclick={() => pick(p.id)}>
            <span>{p.icon}</span> <span class="mi-name">{p.name}</span>
            {#if p.builtin}<span class="badge">vestavěná</span>{/if}
          </button>
          {#if !p.builtin}
            <button class="del" onclick={() => personaStore.remove(p.id)} aria-label="Smazat">×</button>
          {/if}
        </div>
      {/each}

      {#if creating}
        <div class="new-form">
          <div class="nf-row">
            <input class="nf-icon" bind:value={newIcon} maxlength="2" aria-label="Ikona" />
            <input class="nf-name" placeholder="Název persony" bind:value={newName} />
          </div>
          <textarea placeholder="System prompt..." bind:value={newPrompt} rows="3"></textarea>
          <div class="nf-actions">
            <button class="nf-cancel" onclick={() => (creating = false)}>Zrušit</button>
            <button class="nf-save" onclick={submitNew} disabled={!newName.trim() || !newPrompt.trim()}>
              Vytvořit
            </button>
          </div>
        </div>
      {:else}
        <button class="menu-item add" onclick={() => (creating = true)}>＋ Nová persona</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .persona-picker {
    position: relative;
  }

  .persona-btn {
    display: flex;
    align-items: center;
    gap: 0.35rem;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.35rem 0.6rem;
    font-size: 0.82rem;
    color: var(--color-text);
    cursor: pointer;
    transition: border-color 0.15s;
  }
  .persona-btn:hover {
    border-color: var(--color-text-muted);
  }
  .pname.muted {
    color: var(--color-text-muted);
  }
  .caret {
    font-size: 0.65rem;
    color: var(--color-text-muted);
  }

  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    background: transparent;
    border: none;
    cursor: default;
  }

  .persona-menu {
    position: absolute;
    top: 100%;
    left: 0;
    margin-top: 0.4rem;
    z-index: 25;
    width: 280px;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    box-shadow: 0 8px 28px rgba(0, 0, 0, 0.35);
    padding: 0.4rem;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .menu-row {
    display: flex;
    align-items: center;
  }

  .menu-item {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 0.5rem;
    background: transparent;
    border: none;
    color: var(--color-text);
    padding: 0.5rem 0.6rem;
    border-radius: 7px;
    font-size: 0.85rem;
    cursor: pointer;
    text-align: left;
  }
  .menu-item:hover {
    background: var(--color-surface-2);
  }
  .menu-item.active {
    background: var(--color-user-bubble);
  }
  .mi-name.muted {
    color: var(--color-text-muted);
  }
  .menu-item.add {
    color: var(--color-accent);
    font-weight: 600;
  }

  .badge {
    margin-left: auto;
    font-size: 0.68rem;
    color: var(--color-text-muted);
    background: var(--color-surface-2);
    border-radius: 4px;
    padding: 0.05rem 0.35rem;
  }

  .del {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 1rem;
    padding: 0 0.4rem;
  }
  .del:hover {
    color: var(--color-error);
  }

  .new-form {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
    padding: 0.5rem;
    border-top: 1px solid var(--color-border);
    margin-top: 0.25rem;
  }
  .nf-row {
    display: flex;
    gap: 0.4rem;
  }
  .nf-icon {
    width: 44px;
    text-align: center;
  }
  .nf-name {
    flex: 1;
  }
  .new-form input,
  .new-form textarea {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.4rem 0.5rem;
    font-size: 0.82rem;
    outline: none;
    font-family: inherit;
    resize: none;
  }
  .new-form input:focus,
  .new-form textarea:focus {
    border-color: var(--color-accent);
  }
  .nf-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.4rem;
  }
  .nf-cancel,
  .nf-save {
    border: none;
    border-radius: 6px;
    padding: 0.35rem 0.75rem;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
  }
  .nf-cancel {
    background: transparent;
    color: var(--color-text-muted);
    border: 1px solid var(--color-border);
  }
  .nf-save {
    background: var(--color-accent);
    color: #fff;
  }
  .nf-save:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
