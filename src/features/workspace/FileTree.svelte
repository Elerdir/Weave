<script lang="ts">
  import type { WorkspaceEntry } from "$lib/stores/workspace.svelte";
  import FileTreeNode from "./FileTreeNode.svelte";

  let { entries }: { entries: WorkspaceEntry[]; root: string } = $props();
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
