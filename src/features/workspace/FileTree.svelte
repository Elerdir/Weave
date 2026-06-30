<script lang="ts">
  import type { WorkspaceEntry } from "$lib/stores/workspace.svelte";
  import { workspaceStore } from "$lib/stores/workspace.svelte";
  import FileTreeNode from "./FileTreeNode.svelte";

  let { entries, root }: { entries: WorkspaceEntry[]; root: string } = $props();
</script>

<div class="file-tree">
  {#each entries as entry (entry.path)}
    <FileTreeNode {entry} depth={0} />
  {/each}
</div>

<style>
  .file-tree {
    padding: 0.4rem 0;
    font-size: 0.85rem;
  }
</style>
