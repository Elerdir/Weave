<script lang="ts">
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import type { Message } from "$lib/stores/conversations.svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { referenceQueue } from "$lib/stores/reference-queue.svelte";
  import { regenerateResponse } from "$lib/services/chat.service";
  import { extractLocalImagePaths, fileNameFromPath } from "$lib/generated-images";
  import { i18n } from "$lib/i18n/index.svelte";
  import { tts } from "$lib/services/tts.svelte";

  let { msg, isLast = false }: { msg: Message; isLast?: boolean } = $props();

  const isUser = $derived(msg.role === "user");
  const isSpeaking = $derived(tts.speakingId === msg.id);
  const imageAttachments = $derived(msg.attachments.filter((a) => a.type === "image"));
  const canRegenerate = $derived(
    !isUser && isLast && !conversationStore.loading && conversationStore.activeId !== null
  );
  /** Vygenerované obrázky v odpovědi asistenta (lokální cesty z markdownu). */
  const generatedImages = $derived(isUser ? [] : extractLocalImagePaths(msg.content));

  function regenerate() {
    if (conversationStore.activeId) void regenerateResponse(conversationStore.activeId);
  }

  async function saveImage(source: string) {
    const dest = await saveDialog({
      defaultPath: fileNameFromPath(source),
      filters: [{ name: "Obrázek", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (dest) await invoke("save_file_copy", { source, dest });
  }

  function useAsReference(path: string) {
    referenceQueue.add(path);
  }

  // Jednoduchý markdown renderer — inline kód + code bloky
  function renderContent(text: string): string {
    return text
      .replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code class="lang-$1">$2</code></pre>')
      .replace(/`([^`]+)`/g, '<code>$1</code>')
      .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
      .replace(/\*(.+?)\*/g, '<em>$1</em>')
      .replace(
        /!\[([^\]]*)\]\(([^)]+)\)/g,
        (_match, alt: string, path: string) =>
          `<img src="${convertFileSrc(path)}" alt="${alt}" class="inline-image" />`
      )
      .replace(/\n/g, "<br>");
  }

  async function copyContent() {
    await navigator.clipboard.writeText(msg.content);
  }
</script>

<div class="bubble-wrap" class:user={isUser}>
  <div class="bubble" class:user={isUser} class:assistant={!isUser}>
    {#if imageAttachments.length > 0}
      <div class="attachment-thumbs">
        {#each imageAttachments as att (att.path)}
          <img src={convertFileSrc(att.path)} alt="" class="attachment-thumb" />
        {/each}
      </div>
    {/if}

    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
    <div class="bubble-content">{@html renderContent(msg.content)}</div>

    {#if msg.stats}
      <div class="stats">
        {i18n.t("chat.tokensPerSecond", { tps: msg.stats.tokens_per_second.toFixed(1) })}
        · {msg.stats.model_id}
      </div>
    {/if}
  </div>

  <div class="actions">
    <button class="action-btn" onclick={copyContent} title={i18n.m.chat.copy}>⎘</button>
    {#if !isUser && tts.supported}
      <button
        class="action-btn"
        class:speaking={isSpeaking}
        onclick={() => tts.speak(msg.id, msg.content, i18n.locale)}
        title={i18n.m.chat.speak}
        aria-label={i18n.m.chat.speak}
      >{isSpeaking ? "⏹" : "🔊"}</button>
    {/if}
    {#if canRegenerate}
      <button
        class="action-btn"
        onclick={regenerate}
        title={i18n.m.chat.regenerate}
        aria-label={i18n.m.chat.regenerate}
      >↻</button>
    {/if}
    {#if generatedImages.length > 0}
      <button
        class="action-btn"
        onclick={() => saveImage(generatedImages[0])}
        title={i18n.m.chat.saveImage}
        aria-label={i18n.m.chat.saveImage}
      >💾</button>
      <button
        class="action-btn"
        onclick={() => useAsReference(generatedImages[0])}
        title={i18n.m.chat.useAsReference}
        aria-label={i18n.m.chat.useAsReference}
      >🖼️</button>
    {/if}
  </div>
</div>

<style>
  .bubble-wrap {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    max-width: 78%;
  }

  .bubble-wrap.user {
    align-self: flex-end;
    align-items: flex-end;
  }

  .bubble {
    border-radius: 12px;
    padding: 0.75rem 1rem;
    font-size: 0.9rem;
    line-height: 1.7;
    word-break: break-word;
  }

  .bubble.user {
    background: var(--color-user-bubble);
    border: 1px solid var(--color-accent);
    border-bottom-right-radius: 4px;
  }

  .bubble.assistant {
    background: var(--color-assistant-bubble);
    border: 1px solid var(--color-border);
    border-bottom-left-radius: 4px;
  }

  .bubble-content :global(pre) {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.75rem;
    overflow-x: auto;
    font-size: 0.82rem;
    margin: 0.5rem 0;
  }

  .bubble-content :global(code) {
    font-family: "JetBrains Mono", "Fira Code", monospace;
    font-size: 0.85em;
    background: var(--color-surface-2);
    padding: 0.1em 0.3em;
    border-radius: 4px;
  }

  .bubble-content :global(pre code) {
    background: transparent;
    padding: 0;
  }

  .bubble-content :global(.inline-image) {
    max-width: 100%;
    border-radius: 8px;
    margin-top: 0.5rem;
  }

  .attachment-thumbs {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    margin-bottom: 0.5rem;
  }

  .attachment-thumb {
    width: 96px;
    height: 96px;
    object-fit: cover;
    border-radius: 8px;
    border: 1px solid var(--color-border);
  }

  .stats {
    margin-top: 0.4rem;
    font-size: 0.75rem;
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }

  .actions {
    display: flex;
    gap: 0.25rem;
    margin-top: 0.25rem;
    opacity: 0;
    transition: opacity 0.15s;
  }

  .bubble-wrap:hover .actions { opacity: 1; }

  .action-btn {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
    border-radius: 6px;
    padding: 0.2rem 0.4rem;
    font-size: 0.8rem;
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }

  .action-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-2);
  }

  .action-btn.speaking {
    color: var(--color-accent);
    border-color: var(--color-accent);
  }
</style>
