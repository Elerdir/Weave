<script lang="ts">
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { generationSettingsStore } from "$lib/stores/generation-settings.svelte";
  import {
    estimateConversationTokens,
    contextUsagePercent,
    usageSeverity,
  } from "$lib/context-tokens";
  import { i18n } from "$lib/i18n/index.svelte";

  const usedTokens = $derived(
    estimateConversationTokens(conversationStore.messages, conversationStore.activeStreamingContent)
  );
  const contextLength = $derived(generationSettingsStore.contextLength);
  const percent = $derived(contextUsagePercent(usedTokens, contextLength));
  const severity = $derived(usageSeverity(percent));

  // Nabídka zhuštění při téměř plném kontextu — „Později" ji schová pro
  // danou konverzaci (žádné otravování dokola).
  let dismissedFor = $state<string | null>(null);
  const showSuggestion = $derived(
    percent >= 90 &&
      !conversationStore.loading &&
      !conversationStore.compacting &&
      conversationStore.messages.length >= 2 &&
      dismissedFor !== conversationStore.activeId
  );

  async function compact() {
    if (!confirm(i18n.m.chat.contextMeter.compactConfirm)) return;
    await conversationStore.compact();
  }

  /** Z nabídky — uživatel už souhlas vyjádřil kliknutím, bez confirm dialogu. */
  async function compactNow() {
    await conversationStore.compact();
  }

  function dismissSuggestion() {
    dismissedFor = conversationStore.activeId;
  }
</script>

<div class="context-meter" title={i18n.m.chat.contextMeter.tooltip}>
  <div class="meter-bar" role="progressbar" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
    <div class="meter-fill sev-{severity}" style="width: {percent}%"></div>
  </div>
  <span class="meter-text sev-text-{severity}">
    ~{usedTokens.toLocaleString()} / {contextLength.toLocaleString()} ({percent} %)
  </span>
  <button
    class="compact-btn"
    onclick={compact}
    disabled={conversationStore.compacting ||
      conversationStore.loading ||
      conversationStore.messages.length < 2}
    title={i18n.m.chat.contextMeter.compactHint}
  >
    {conversationStore.compacting
      ? i18n.m.chat.contextMeter.compacting
      : i18n.m.chat.contextMeter.compact}
  </button>
</div>

{#if showSuggestion}
  <div class="compact-suggestion" role="status">
    <span>⚠ {i18n.m.chat.contextMeter.fullWarning}</span>
    <div class="suggestion-actions">
      <button class="suggestion-yes" onclick={compactNow}>
        {i18n.m.chat.contextMeter.compactNow}
      </button>
      <button class="suggestion-later" onclick={dismissSuggestion}>
        {i18n.m.chat.contextMeter.later}
      </button>
    </div>
  </div>
{/if}

<style>
  .context-meter {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0.35rem 1.25rem;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
    font-size: 0.75rem;
  }

  .meter-bar {
    flex: 1;
    height: 6px;
    background: var(--color-surface-2);
    border-radius: 3px;
    overflow: hidden;
  }

  .meter-fill {
    height: 100%;
    border-radius: 3px;
    transition: width 0.3s ease;
  }

  .sev-ok { background: var(--color-accent); }
  .sev-warn { background: #d29922; }
  .sev-danger { background: #e5534b; }

  .meter-text {
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .sev-text-warn { color: #d29922; }
  .sev-text-danger { color: #e5534b; font-weight: 600; }

  .compact-btn {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
    border-radius: 6px;
    padding: 0.15rem 0.55rem;
    font-size: 0.75rem;
    cursor: pointer;
    white-space: nowrap;
    transition: color 0.15s, border-color 0.15s;
  }

  .compact-btn:hover:not(:disabled) {
    color: var(--color-accent);
    border-color: var(--color-accent);
  }

  .compact-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .compact-suggestion {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.75rem;
    padding: 0.45rem 1.25rem;
    border-bottom: 1px solid #d29922;
    background: color-mix(in srgb, #d29922 12%, transparent);
    font-size: 0.8rem;
    color: var(--color-text);
  }

  .suggestion-actions {
    display: flex;
    gap: 0.4rem;
  }

  .suggestion-yes {
    background: var(--color-accent);
    color: #fff;
    border: none;
    border-radius: 6px;
    padding: 0.2rem 0.7rem;
    font-size: 0.78rem;
    font-weight: 600;
    cursor: pointer;
  }
  .suggestion-yes:hover {
    background: var(--color-accent-hover);
  }

  .suggestion-later {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
    border-radius: 6px;
    padding: 0.2rem 0.7rem;
    font-size: 0.78rem;
    cursor: pointer;
  }
  .suggestion-later:hover {
    color: var(--color-text);
  }
</style>
