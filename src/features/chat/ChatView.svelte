<script lang="ts">
  import { tick } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
  import { filterNewImagePaths, IMAGE_EXTENSIONS } from "$lib/reference-images";
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { generationSettingsStore } from "$lib/stores/generation-settings.svelte";
  import { sendMessage, stopGeneration } from "$lib/services/chat.service";
  import { i18n } from "$lib/i18n/index.svelte";
  import { activeMention, removeMentionToken } from "$lib/mentions";
  import type { MentionMatch } from "$lib/mentions";
  import type { IndexedFile } from "$lib/stores/workspace.svelte";
  import MessageBubble from "./MessageBubble.svelte";
  import PersonaPicker from "./PersonaPicker.svelte";
  import ExportMenu from "./ExportMenu.svelte";

  interface Mention {
    path: string;
    name: string;
  }

  interface RefImage {
    path: string;
    previewUrl: string;
  }

  let input = $state("");
  let messagesEl = $state<HTMLDivElement | null>(null);
  let inputEl = $state<HTMLTextAreaElement | null>(null);

  // @soubor reference
  let mentions = $state<Mention[]>([]);
  let suggestions = $state<IndexedFile[]>([]);
  let activeMatch = $state<MentionMatch | null>(null);
  let highlighted = $state(0);
  let searchTimer: ReturnType<typeof setTimeout>;

  // Referenční obrázky pro generování (náhled hned po výběru)
  let refImages = $state<RefImage[]>([]);

  // Panel per-konverzačních parametrů generování (posuvníky)
  let showGenSettings = $state(false);

  $effect(() => {
    const id = conversationStore.activeId;
    if (id) {
      showGenSettings = false;
      void generationSettingsStore.load(id);
    }
  });

  function addReferenceImages(paths: string[]) {
    const fresh = filterNewImagePaths(
      paths,
      refImages.map((r) => r.path)
    );
    if (fresh.length > 0) {
      refImages = [
        ...refImages,
        ...fresh.map((path) => ({ path, previewUrl: convertFileSrc(path) })),
      ];
    }
  }

  async function pickReferenceImages() {
    const picked = await openFilePicker({
      multiple: true,
      filters: [{ name: "Obrázky", extensions: IMAGE_EXTENSIONS }],
    });
    if (!picked) return;
    addReferenceImages(Array.isArray(picked) ? picked : [picked]);
  }

  function removeRefImage(path: string) {
    refImages = refImages.filter((r) => r.path !== path);
  }

  // Drag & drop obrázků kamkoliv do okna chatu → referenční obrázky
  let dragActive = $state(false);

  $effect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          dragActive = true;
        } else if (event.payload.type === "drop") {
          dragActive = false;
          addReferenceImages(event.payload.paths);
        } else {
          dragActive = false; // leave
        }
      })
      .then((u) => {
        if (disposed) u();
        else unlisten = u;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  $effect(() => {
    void conversationStore.messages.length;
    void conversationStore.streamingContent;
    tick().then(() => {
      if (messagesEl) messagesEl.scrollTop = messagesEl.scrollHeight;
    });
  });

  function closeSuggestions() {
    suggestions = [];
    activeMatch = null;
    highlighted = 0;
  }

  function onInput() {
    if (!inputEl) return;
    const cursor = inputEl.selectionStart ?? input.length;
    const match = activeMention(input, cursor);

    if (!match || match.query.length < 1) {
      closeSuggestions();
      return;
    }

    activeMatch = match;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(async () => {
      try {
        const results = await invoke<IndexedFile[]>("search_workspace", {
          query: match.query,
          limit: 6,
        });
        suggestions = results;
        highlighted = 0;
      } catch {
        suggestions = [];
      }
    }, 200);
  }

  function pickSuggestion(file: IndexedFile) {
    if (!mentions.some((m) => m.path === file.path)) {
      mentions = [...mentions, { path: file.path, name: file.name }];
    }
    if (activeMatch) {
      input = removeMentionToken(input, activeMatch);
    }
    closeSuggestions();
    inputEl?.focus();
  }

  function removeMention(path: string) {
    mentions = mentions.filter((m) => m.path !== path);
  }

  async function submit() {
    const content = input.trim();
    if (!content || conversationStore.loading || !conversationStore.activeId) return;
    const refs = mentions.map((m) => m.path);
    const images = refImages.map((r) => r.path);
    input = "";
    mentions = [];
    refImages = [];
    closeSuggestions();
    await sendMessage(conversationStore.activeId, content, refs, images);
  }

  function onKeydown(e: KeyboardEvent) {
    if (suggestions.length > 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        highlighted = (highlighted + 1) % suggestions.length;
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        highlighted = (highlighted - 1 + suggestions.length) % suggestions.length;
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        pickSuggestion(suggestions[highlighted]);
        return;
      }
      if (e.key === "Escape") {
        closeSuggestions();
        return;
      }
    }

    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  }
</script>

<div class="chat-view">
  {#if dragActive}
    <div class="drop-overlay">
      <div class="drop-overlay-inner">🖼️ {i18n.m.chat.dropImagesHint}</div>
    </div>
  {/if}
  <header class="chat-header">
    <div class="header-left">
      <span class="conv-title">
        {conversationStore.activeConversation?.title ?? ""}
      </span>
      <PersonaPicker />
    </div>
    <div class="header-right">
      {#if conversationStore.currentStats}
        <span class="tps-badge">
          {i18n.t("chat.tokensPerSecond", {
            tps: conversationStore.currentStats.tokens_per_second.toFixed(1)
          })} · {conversationStore.currentStats.model_id}
        </span>
      {/if}
      <button
        class="gen-settings-btn"
        class:active={showGenSettings}
        onclick={() => (showGenSettings = !showGenSettings)}
        title={i18n.m.chat.genSettings.title}
        aria-label={i18n.m.chat.genSettings.title}
      >⚙</button>
      <ExportMenu />
    </div>
  </header>

  {#if showGenSettings}
    <div class="gen-settings-panel">
      <div class="gen-field">
        <div class="gen-label-row">
          <label for="gen-context">{i18n.m.chat.genSettings.context}</label>
          <span class="gen-value">{generationSettingsStore.contextLength.toLocaleString()}</span>
        </div>
        <input
          id="gen-context"
          type="range"
          min="2048"
          max="32768"
          step="1024"
          value={generationSettingsStore.contextLength}
          oninput={(e) =>
            generationSettingsStore.setContextLength(Number((e.target as HTMLInputElement).value))}
          onchange={() => generationSettingsStore.save()}
        />
      </div>

      <div class="gen-field">
        <div class="gen-label-row">
          <label for="gen-temperature">{i18n.m.chat.genSettings.temperature}</label>
          <span class="gen-value">{generationSettingsStore.temperature.toFixed(2)}</span>
        </div>
        <input
          id="gen-temperature"
          type="range"
          min="0"
          max="2"
          step="0.05"
          value={generationSettingsStore.temperature}
          oninput={(e) =>
            generationSettingsStore.setTemperature(Number((e.target as HTMLInputElement).value))}
          onchange={() => generationSettingsStore.save()}
        />
      </div>

      <div class="gen-field">
        <div class="gen-label-row">
          <label for="gen-max-tokens">{i18n.m.chat.genSettings.maxTokens}</label>
          <span class="gen-value">
            {generationSettingsStore.maxTokens > 0
              ? generationSettingsStore.maxTokens.toLocaleString()
              : i18n.m.chat.genSettings.unlimited}
          </span>
        </div>
        <input
          id="gen-max-tokens"
          type="range"
          min="0"
          max="8192"
          step="256"
          value={generationSettingsStore.maxTokens}
          oninput={(e) =>
            generationSettingsStore.setMaxTokens(Number((e.target as HTMLInputElement).value))}
          onchange={() => generationSettingsStore.save()}
        />
      </div>
    </div>
  {/if}

  <div class="messages" bind:this={messagesEl}>
    {#each conversationStore.messages as msg, i (msg.id)}
      <MessageBubble {msg} isLast={i === conversationStore.messages.length - 1} />
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

  <div class="input-area-wrap">
    {#if conversationStore.lastError}
      <div class="error-banner" role="alert">
        <span class="error-text">⚠️ {conversationStore.lastError}</span>
        <button
          class="error-dismiss"
          onclick={() => conversationStore.setLastError(null)}
          aria-label="Zavřít"
        >×</button>
      </div>
    {/if}

    {#if refImages.length > 0}
      <div class="ref-image-strip">
        {#each refImages as img (img.path)}
          <div class="ref-image-thumb">
            <img src={img.previewUrl} alt="" />
            <button
              class="ref-image-remove"
              onclick={() => removeRefImage(img.path)}
              aria-label={i18n.m.chat.removeReferenceImage}
            >×</button>
          </div>
        {/each}
      </div>
    {/if}

    {#if mentions.length > 0}
      <div class="mention-chips">
        {#each mentions as m (m.path)}
          <span class="chip">
            📎 {m.name}
            <button class="chip-x" onclick={() => removeMention(m.path)} aria-label="Odebrat">×</button>
          </span>
        {/each}
      </div>
    {/if}

    {#if suggestions.length > 0}
      <div class="mention-popup" role="listbox">
        {#each suggestions as file, i (file.path)}
          <button
            class="mention-item"
            class:active={i === highlighted}
            onmousedown={(e) => { e.preventDefault(); pickSuggestion(file); }}
          >
            <span class="mention-name">{file.name}</span>
            <span class="mention-path">{file.path}</span>
          </button>
        {/each}
      </div>
    {/if}

    <div class="input-area">
      <button
        class="attach-btn"
        onclick={pickReferenceImages}
        disabled={conversationStore.loading}
        title={i18n.m.chat.addReferenceImage}
        aria-label={i18n.m.chat.addReferenceImage}
      >🖼️</button>
      <textarea
        class="chat-input"
        bind:this={inputEl}
        placeholder={i18n.m.chat.placeholder}
        bind:value={input}
        oninput={onInput}
        onkeydown={onKeydown}
        disabled={conversationStore.loading}
        rows="1"
      ></textarea>
      {#if conversationStore.loading}
        <button class="stop-btn" onclick={() => stopGeneration()}>
          {i18n.m.chat.stop}
        </button>
      {:else}
        <button
          class="send-btn"
          onclick={submit}
          disabled={!input.trim()}
        >
          {i18n.m.chat.send}
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .chat-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    position: relative;
  }

  .drop-overlay {
    position: absolute;
    inset: 0;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, var(--color-bg) 70%, transparent);
    border: 2px dashed var(--color-accent);
    border-radius: 12px;
    margin: 0.5rem;
    pointer-events: none;
  }

  .drop-overlay-inner {
    font-size: 1.1rem;
    font-weight: 600;
    color: var(--color-accent);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 12px;
    padding: 0.85rem 1.5rem;
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

  .header-left {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .header-right {
    display: flex;
    align-items: center;
    gap: 0.75rem;
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

  .gen-settings-btn {
    background: transparent;
    border: 1px solid var(--color-border);
    color: var(--color-text-muted);
    border-radius: 8px;
    padding: 0.25rem 0.5rem;
    font-size: 0.9rem;
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }
  .gen-settings-btn:hover {
    color: var(--color-text);
    background: var(--color-surface-2);
  }
  .gen-settings-btn.active {
    color: var(--color-accent);
    border-color: var(--color-accent);
  }

  .gen-settings-panel {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem 2rem;
    padding: 0.85rem 1.25rem;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .gen-field {
    flex: 1 1 220px;
    min-width: 200px;
  }

  .gen-label-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 0.25rem;
  }

  .gen-label-row label {
    font-size: 0.78rem;
    color: var(--color-text-muted);
  }

  .gen-value {
    font-size: 0.8rem;
    font-weight: 600;
    color: var(--color-accent);
    font-variant-numeric: tabular-nums;
  }

  .gen-field input[type="range"] {
    width: 100%;
    accent-color: var(--color-accent);
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

  .input-area-wrap {
    position: relative;
    border-top: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  .error-banner {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.75rem;
    margin: 0.75rem 1.25rem 0;
    padding: 0.6rem 0.85rem;
    background: color-mix(in srgb, var(--color-error, #e5484d) 10%, transparent);
    border: 1px solid var(--color-error, #e5484d);
    border-radius: 10px;
    font-size: 0.82rem;
    color: var(--color-text);
  }

  .error-banner .error-text {
    line-height: 1.5;
    word-break: break-word;
  }

  .error-dismiss {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    padding: 0 0.15rem;
  }
  .error-dismiss:hover { color: var(--color-text); }

  .ref-image-strip {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    padding: 0.75rem 1.25rem 0;
  }

  .ref-image-thumb {
    position: relative;
    width: 56px;
    height: 56px;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--color-border);
  }

  .ref-image-thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }

  .ref-image-remove {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 18px;
    height: 18px;
    line-height: 1;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
    border: none;
    border-radius: 50%;
    cursor: pointer;
    font-size: 0.75rem;
  }

  .ref-image-remove:hover {
    background: rgba(0, 0, 0, 0.85);
  }

  .attach-btn {
    background: transparent;
    border: 1px solid var(--color-border);
    border-radius: 10px;
    padding: 0.5rem 0.65rem;
    font-size: 1rem;
    cursor: pointer;
    align-self: flex-end;
    transition: background 0.2s;
  }

  .attach-btn:hover:not(:disabled) { background: var(--color-surface-2); }
  .attach-btn:disabled { opacity: 0.45; cursor: default; }

  .mention-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.4rem;
    padding: 0.6rem 1.25rem 0;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    background: var(--color-user-bubble);
    border: 1px solid var(--color-accent);
    border-radius: 6px;
    padding: 0.2rem 0.5rem;
    font-size: 0.78rem;
  }

  .chip-x {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 0.9rem;
    line-height: 1;
    padding: 0;
  }
  .chip-x:hover {
    color: var(--color-text);
  }

  .mention-popup {
    position: absolute;
    bottom: 100%;
    left: 1.25rem;
    right: 1.25rem;
    margin-bottom: 0.4rem;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.3);
    overflow: hidden;
    z-index: 30;
    max-height: 240px;
    overflow-y: auto;
  }

  .mention-item {
    display: flex;
    flex-direction: column;
    gap: 0.1rem;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    padding: 0.5rem 0.85rem;
    cursor: pointer;
  }
  .mention-item:hover,
  .mention-item.active {
    background: var(--color-surface-2);
  }

  .mention-name {
    font-size: 0.85rem;
    font-weight: 600;
    color: var(--color-text);
  }
  .mention-path {
    font-size: 0.72rem;
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .input-area {
    display: flex;
    gap: 0.75rem;
    padding: 1rem 1.25rem;
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

  .stop-btn {
    background: transparent;
    color: var(--color-error, #e5484d);
    border: 1px solid var(--color-error, #e5484d);
    border-radius: 10px;
    padding: 0.65rem 1.25rem;
    font-size: 0.875rem;
    font-weight: 600;
    cursor: pointer;
    align-self: flex-end;
    transition: background 0.2s;
    white-space: nowrap;
  }

  .stop-btn:hover { background: color-mix(in srgb, var(--color-error, #e5484d) 12%, transparent); }
</style>
