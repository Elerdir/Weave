<script lang="ts">
  import { onMount, tick } from "svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { sendMessage } from "$lib/services/chat.service";
  import { i18n } from "$lib/i18n/index.svelte";
  import MessageBubble from "./MessageBubble.svelte";

  let input = $state("");
  let messagesEl = $state<HTMLDivElement | null>(null);

  $effect(() => {
    // Scroll na konec při každé nové zprávě nebo streamu
    const _ = conversationStore.messages.length + conversationStore.streamingContent;
    tick().then(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    });
  });

  async function submit() {
    const content = input.trim();
    if (!content || conversationStore.loading || !conversationStore.activeId) return;
    input = "";
    await sendMessage(conversationStore.activeId, content);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }
</script>

<div class="chat-view">
  <header class="chat-header">
    <span class="conv-title">
      {conversationStore.activeConversation?.title ?? ""}
    </span>
    {#if conversationStore.currentStats}
      <span class="tps-badge">
        {i18n.t("chat.tokensPerSecond", {
          tps: conversationStore.currentStats.tokens_per_second.toFixed(1)
        })} · {conversationStore.currentStats.model_id}
      </span>
    {/if}
  </header>

  <div class="messages" bind:this={messagesEl}>
    {#each conversationStore.messages as msg (msg.id)}
      <MessageBubble {msg} />
    {/each}

    {#if conversationStore.streamingContent !== null}
      <div class="bubble assistant streaming">
        <div class="bubble-content">
          {conversationStore.streamingContent}
          <span class="cursor"></span>
        </div>
      </div>
    {:else if conversationStore.loading}
      <div class="bubble assistant thinking">
        <span class="dot"></span>
        <span class="dot"></span>
        <span class="dot"></span>
      </div>
    {/if}
  </div>

  <div class="input-area">
    <textarea
      class="chat-input"
      placeholder={i18n.m.chat.placeholder}
      bind:value={input}
      onkeydown={onKeydown}
      disabled={conversationStore.loading}
      rows="1"
    ></textarea>
    <button
      class="send-btn"
      onclick={submit}
      disabled={!input.trim() || conversationStore.loading}
    >
      {i18n.m.chat.send}
    </button>
  </div>
</div>

<style>
  .chat-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .chat-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0.75rem 1.25rem;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    min-height: 48px;
  }

  .conv-title {
    font-weight: 600;
    font-size: 0.95rem;
    color: var(--color-text);
  }

  .tps-badge {
    font-size: 0.78rem;
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }

  .messages {
    flex: 1;
    overflow-y: auto;
    padding: 1.25rem;
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .bubble {
    max-width: 75%;
    border-radius: 12px;
    padding: 0.75rem 1rem;
    font-size: 0.9rem;
    line-height: 1.7;
  }

  .bubble.assistant {
    align-self: flex-start;
    background: var(--color-assistant-bubble);
    border: 1px solid var(--color-border);
  }

  .bubble.thinking {
    display: flex;
    gap: 0.3rem;
    align-items: center;
    padding: 0.75rem 1rem;
  }

  .dot {
    width: 7px;
    height: 7px;
    background: var(--color-text-muted);
    border-radius: 50%;
    animation: blink 1.2s infinite;
  }

  .dot:nth-child(2) { animation-delay: 0.2s; }
  .dot:nth-child(3) { animation-delay: 0.4s; }

  @keyframes blink {
    0%, 80%, 100% { opacity: 0.2; }
    40% { opacity: 1; }
  }

  .cursor {
    display: inline-block;
    width: 2px;
    height: 1em;
    background: var(--color-accent);
    margin-left: 2px;
    vertical-align: text-bottom;
    animation: blink-cursor 0.8s steps(1) infinite;
  }

  @keyframes blink-cursor {
    50% { opacity: 0; }
  }

  .input-area {
    display: flex;
    gap: 0.75rem;
    padding: 1rem 1.25rem;
    border-top: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .chat-input {
    flex: 1;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    padding: 0.65rem 0.9rem;
    font-size: 0.9rem;
    font-family: inherit;
    resize: none;
    outline: none;
    line-height: 1.5;
    field-sizing: content;
    max-height: 160px;
  }

  .chat-input:focus { border-color: var(--color-accent); }

  .send-btn {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 10px;
    padding: 0.65rem 1.25rem;
    font-size: 0.875rem;
    font-weight: 600;
    cursor: pointer;
    align-self: flex-end;
    transition: background 0.2s;
    white-space: nowrap;
  }

  .send-btn:hover:not(:disabled) { background: var(--color-accent-hover); }
  .send-btn:disabled { opacity: 0.45; cursor: default; }
</style>
