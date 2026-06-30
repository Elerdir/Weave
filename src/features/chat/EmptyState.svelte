<script lang="ts">
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { i18n } from "$lib/i18n/index.svelte";
</script>

<div class="empty">
  <div class="logo">Weave</div>
  <p class="tagline">Tvůj inteligentní AI asistent</p>
  <button class="btn-start" onclick={() => conversationStore.create(i18n.m.chat.newConversation)}>
    {i18n.m.chat.newConversation}
  </button>
</div>

<style>
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1rem;
    color: var(--color-text-muted);
  }

  .logo {
    font-size: 3rem;
    font-weight: 800;
    color: var(--color-accent);
    letter-spacing: 0.1em;
    opacity: 0.7;
  }

  .tagline {
    font-size: 1rem;
  }

  .btn-start {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 10px;
    padding: 0.75rem 2rem;
    font-size: 0.95rem;
    font-weight: 600;
    cursor: pointer;
    margin-top: 0.5rem;
    transition: background 0.2s;
  }

  .btn-start:hover { background: var(--color-accent-hover); }
</style>
