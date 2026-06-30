<script lang="ts">
  import Sidebar from "./Sidebar.svelte";
  import ChatView from "./ChatView.svelte";
  import EmptyState from "./EmptyState.svelte";
  import WorkspacePanel from "$features/workspace/WorkspacePanel.svelte";
  import Settings from "$features/settings/Settings.svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";

  let showWorkspace = $state(false);
  let showSettings = $state(false);
</script>

<div class="layout">
  <Sidebar bind:showWorkspace onOpenSettings={() => (showSettings = true)} />

  {#if showWorkspace}
    <div class="workspace-pane">
      <WorkspacePanel />
    </div>
  {/if}

  <main class="main">
    {#if conversationStore.activeId}
      <ChatView />
    {:else}
      <EmptyState />
    {/if}
  </main>
</div>

{#if showSettings}
  <Settings onClose={() => (showSettings = false)} />
{/if}

<style>
  .layout {
    display: flex;
    height: 100vh;
    overflow: hidden;
  }

  .workspace-pane {
    width: 240px;
    min-width: 180px;
    max-width: 360px;
    display: flex;
    flex-direction: column;
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
