<script lang="ts">
  import Sidebar from "./Sidebar.svelte";
  import ChatView from "./ChatView.svelte";
  import EmptyState from "./EmptyState.svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";
</script>

<div class="layout">
  <Sidebar />
  <main class="main">
    {#if conversationStore.activeId}
      <ChatView />
    {:else}
      <EmptyState />
    {/if}
  </main>
</div>

<style>
  .layout {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--color-bg);
  }
</style>
