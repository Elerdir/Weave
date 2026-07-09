<script lang="ts">
  import { tick } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
  import { filterNewImagePaths, IMAGE_EXTENSIONS } from "$lib/reference-images";
  import { referenceQueue } from "$lib/stores/reference-queue.svelte";
  import { editImageStore } from "$lib/stores/edit-image.svelte";
  import { subjectsStore, type Subject } from "$lib/stores/subjects.svelte";
  import { conversationStore } from "$lib/stores/conversations.svelte";
  import { generationSettingsStore } from "$lib/stores/generation-settings.svelte";
  import type { RuntimeBackend } from "$lib/stores/generation-settings.svelte";
  import {
    editImageMessage,
    regenerateResponse,
    sendMessage,
    stopGeneration,
  } from "$lib/services/chat.service";
  import { fileNameFromPath } from "$lib/generated-images";
  import { i18n } from "$lib/i18n/index.svelte";
  import { activeMention, removeMentionToken } from "$lib/mentions";
  import type { MentionMatch } from "$lib/mentions";
  import type { IndexedFile } from "$lib/stores/workspace.svelte";
  import MessageBubble from "./MessageBubble.svelte";
  import PersonaPicker from "./PersonaPicker.svelte";
  import ExportMenu from "./ExportMenu.svelte";
  import ContextMeter from "./ContextMeter.svelte";

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
  const MAX_REFERENCE_IMAGES = 4;
  let refImages = $state<RefImage[]>([]);
  let referencePreservation = $state("");
  let translateImagePrompt = $state(true);

  // Panel per-konverzačních parametrů generování (posuvníky)
  let showGenSettings = $state(false);
  let unloadingModel = $state(false);
  let unloadNotice = $state<string | null>(null);
  let lastActiveConversationId = $state<string | null>(null);
  const runtimeOptions: { value: RuntimeBackend; label: () => string }[] = [
    { value: "default", label: () => i18n.m.chat.runtime.default },
    { value: "embedded", label: () => i18n.m.chat.runtime.gpu },
    { value: "local", label: () => i18n.m.chat.runtime.ram },
    { value: "openvino_npu", label: () => i18n.m.chat.runtime.npu },
    { value: "mistral", label: () => i18n.m.chat.runtime.api },
  ];

  const runtimeInfo = $derived.by(() => {
    const backend = conversationStore.currentStats?.backend ?? "unknown";
    if (backend === "local_cuda" || backend === "local_metal" || backend === "local_vulkan") {
      return { label: i18n.m.chat.runtime.gpu, kind: "gpu" };
    }
    if (backend === "local_cpu") {
      return { label: i18n.m.chat.runtime.ram, kind: "ram" };
    }
    if (backend === "openvino_npu") {
      return { label: i18n.m.chat.runtime.npu, kind: "npu" };
    }
    if (backend === "mistral_api") {
      return { label: i18n.m.chat.runtime.api, kind: "api" };
    }
    if (backend === "comfy_ui") {
      return { label: i18n.m.chat.runtime.comfyui, kind: "api" };
    }
    return { label: i18n.m.chat.runtime.unknown, kind: "unknown" };
  });

  const imagePromptAssistVisible = $derived.by(() => {
    const lower = input.toLowerCase();
    const hasCzechChars = /[áčďéěíňóřšťúůýž]/i.test(input);
    const asksForImage = [
      "obraz",
      "obrazek",
      "obrázek",
      "fotk",
      "nakresli",
      "vygeneruj",
      "ilustrac",
      "portrait",
      "image",
      "photo",
    ].some((word) => lower.includes(word));
    return refImages.length > 0 || hasCzechChars || asksForImage;
  });

  // Výběr referenčních postav (uložené sady fotek)
  let showSubjects = $state(false);
  $effect(() => {
    void subjectsStore.load();
  });

  function attachSubject(subject: Subject) {
    addReferenceImages(subject.images.map((i) => i.path));
    showSubjects = false;
  }

  // Uplynulý čas během přípravy/generování obrázku
  let elapsedSeconds = $state(0);
  const elapsedLabel = $derived(
    elapsedSeconds >= 60
      ? `${Math.floor(elapsedSeconds / 60)} min ${elapsedSeconds % 60} s`
      : `${elapsedSeconds} s`
  );

  $effect(() => {
    if (!(conversationStore.loading && conversationStore.imageStage)) {
      elapsedSeconds = 0;
      return;
    }
    const interval = setInterval(() => {
      elapsedSeconds += 1;
    }, 1000);
    return () => clearInterval(interval);
  });

  $effect(() => {
    const id = conversationStore.activeId;
    if (id !== lastActiveConversationId) {
      lastActiveConversationId = id;
      mentions = [];
      refImages = [];
      referencePreservation = "";
      showSubjects = false;
      closeSuggestions();
    }
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
    const freeSlots = Math.max(0, MAX_REFERENCE_IMAGES - refImages.length);
    const accepted = fresh.slice(0, freeSlots);
    if (accepted.length > 0) {
      refImages = [
        ...refImages,
        ...accepted.map((path) => ({ path, previewUrl: convertFileSrc(path) })),
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
    if (refImages.length === 0) referencePreservation = "";
  }

  // „Použít jako referenci“ z bubliny zprávy → přilož k dalšímu vstupu
  $effect(() => {
    if (referenceQueue.pending.length > 0) {
      addReferenceImages(referenceQueue.drain());
    }
  });

  // „Použít jako referenci" z okna galerie (emitTo("main", "use-reference", path))
  $effect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<string>("use-reference", (e) => {
      if (typeof e.payload === "string") addReferenceImages([e.payload]);
    }).then((u) => {
      if (disposed) u();
      else unlisten = u;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

  // Drag & drop obrázků kamkoliv do okna chatu → referenční obrázky
  $effect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    listen<string>("use-prompt", (e) => {
      if (typeof e.payload !== "string" || e.payload.trim().length === 0) return;
      input = input.trim().length > 0 ? `${input.trim()}\n\n${e.payload}` : e.payload;
      tick().then(() => inputEl?.focus());
    }).then((u) => {
      if (disposed) u();
      else unlisten = u;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  });

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
    void conversationStore.activeStreamingContent;
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

  async function unloadModelMemory() {
    if (unloadingModel || conversationStore.loading) return;
    unloadingModel = true;
    unloadNotice = null;
    try {
      await invoke("unload_embedded_model");
      unloadNotice = i18n.m.chat.runtime.unloaded;
      setTimeout(() => {
        unloadNotice = null;
      }, 3500);
    } catch (e) {
      conversationStore.setLastError(String(e));
    } finally {
      unloadingModel = false;
    }
  }

  async function retryLastGeneration() {
    if (!conversationStore.activeId || conversationStore.loading) return;
    conversationStore.setLastError(null);
    await regenerateResponse(conversationStore.activeId);
  }

  function switchBackendFromError() {
    conversationStore.setLastError(null);
    showGenSettings = true;
  }

  async function openLogsFromError() {
    try {
      await invoke("open_log_window");
    } catch (e) {
      conversationStore.setLastError(String(e));
    }
  }

  async function submit() {
    const content = input.trim();
    if (!content || conversationStore.loading || !conversationStore.activeId) return;
    const conversationId = conversationStore.activeId;
    const refs = mentions.map((m) => m.path);
    const images = refImages.map((r) => r.path);
    const preserve = referencePreservation.trim();
    // Čeká-li obrázek na úpravu, jde instrukce jako img2img místo běžné zprávy.
    const initImage = editImageStore.take();
    const draft = {
      input,
      mentions,
      refImages,
      referencePreservation,
    };

    input = "";
    mentions = [];
    refImages = [];
    referencePreservation = "";
    closeSuggestions();
    try {
      if (initImage) {
        await editImageMessage(conversationId, content, initImage);
      } else {
        await sendMessage(
          conversationId,
          content,
          refs,
          images,
          preserve || null,
          translateImagePrompt
        );
      }
    } catch {
      input = draft.input;
      mentions = draft.mentions;
      refImages = draft.refImages;
      referencePreservation = draft.referencePreservation;
      if (initImage) editImageStore.set(initImage);
    }
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
        <div class="runtime-cluster">
          <span
            class="runtime-badge"
            class:gpu={runtimeInfo.kind === "gpu"}
            class:ram={runtimeInfo.kind === "ram"}
            class:npu={runtimeInfo.kind === "npu"}
          >
            {runtimeInfo.label}
          </span>
          <span class="tps-badge">
            {i18n.t("chat.tokensPerSecond", {
              tps: conversationStore.currentStats.tokens_per_second.toFixed(1)
            })} · {conversationStore.currentStats.model_id}
          </span>
        </div>
      {/if}
      {#if unloadNotice}
        <span class="unload-notice">{unloadNotice}</span>
      {/if}
      <button
        class="gen-settings-btn"
        onclick={unloadModelMemory}
        disabled={conversationStore.loading || unloadingModel}
        title={i18n.m.chat.runtime.unload}
        aria-label={i18n.m.chat.runtime.unload}
      >{unloadingModel ? "..." : "⏏"}</button>
      <button
        class="gen-settings-btn"
        onclick={() => invoke("open_gallery_window").catch((e) => console.warn(e))}
        title={i18n.m.gallery.title}
        aria-label={i18n.m.gallery.title}
      >🖼</button>
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

  <ContextMeter />

  {#if showGenSettings}
    <div class="gen-settings-panel">
      <div class="gen-field runtime-select-field">
        <div class="gen-label-row">
          <label for="gen-runtime">{i18n.m.chat.runtime.override}</label>
          <span class="gen-value">{i18n.m.chat.runtime.perChat}</span>
        </div>
        <select
          id="gen-runtime"
          class="runtime-select"
          value={generationSettingsStore.runtimeBackend}
          onchange={(e) => {
            generationSettingsStore.setRuntimeBackend(
              (e.target as HTMLSelectElement).value as RuntimeBackend
            );
            void generationSettingsStore.save();
          }}
        >
          {#each runtimeOptions as option (option.value)}
            <option value={option.value}>{option.label()}</option>
          {/each}
        </select>
        <p class="gen-hint">{i18n.m.chat.runtime.overrideHint}</p>
      </div>

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

      <div class="gen-section">{i18n.m.chat.genSettings.imageFidelity}</div>

      <div class="gen-field">
        <div class="gen-label-row">
          <label for="gen-pulid">{i18n.m.chat.genSettings.pulidWeight}</label>
          <span class="gen-value">{generationSettingsStore.pulidWeight.toFixed(2)}</span>
        </div>
        <input
          id="gen-pulid"
          type="range"
          min="0.4"
          max="1.4"
          step="0.05"
          value={generationSettingsStore.pulidWeight}
          oninput={(e) =>
            generationSettingsStore.setPulidWeight(Number((e.target as HTMLInputElement).value))}
          onchange={() => generationSettingsStore.save()}
        />
        <p class="gen-hint">{i18n.m.chat.genSettings.pulidWeightHint}</p>
      </div>

      <div class="gen-field">
        <label class="gen-toggle">
          <input
            type="checkbox"
            checked={generationSettingsStore.faceDetailer}
            onchange={(e) => {
              generationSettingsStore.setFaceDetailer((e.target as HTMLInputElement).checked);
              void generationSettingsStore.save();
            }}
          />
          <span>{i18n.m.chat.genSettings.faceDetailer}</span>
        </label>
        <p class="gen-hint">{i18n.m.chat.genSettings.faceDetailerHint}</p>
      </div>
    </div>
  {/if}

  <div class="messages" bind:this={messagesEl}>
    {#each conversationStore.messages as msg, i (msg.id)}
      <MessageBubble {msg} isLast={i === conversationStore.messages.length - 1} />
    {/each}

    {#if conversationStore.activeStreamingContent !== null}
      <div class="bubble assistant streaming">
        <div class="bubble-content">
          {conversationStore.activeStreamingContent}
          <span class="cursor"></span>
        </div>
      </div>
    {:else if conversationStore.isActiveGeneration && conversationStore.activeImageStage}
      <div class="bubble assistant image-progress">
        <div class="image-progress-head">
          <span class="image-progress-label">
            {i18n.t(`chat.imageStages.${conversationStore.activeImageStage.stage}`)}
          </span>
          <span class="image-progress-elapsed">{elapsedLabel}</span>
        </div>
        <div class="image-progress-bar">
          {#if conversationStore.activeImageStage.percent != null}
            <div
              class="image-progress-fill determinate"
              style="width: {conversationStore.activeImageStage.percent}%"
            ></div>
          {:else}
            <div class="image-progress-fill"></div>
          {/if}
        </div>
        {#if conversationStore.activeImageStage.detail}
          <div class="image-progress-detail">{conversationStore.activeImageStage.detail}</div>
        {/if}
      </div>
    {:else if conversationStore.isActiveGeneration}
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
        <div class="error-actions">
          <button onclick={retryLastGeneration} disabled={conversationStore.loading}>
            {i18n.m.chat.errors.retry}
          </button>
          <button onclick={switchBackendFromError}>{i18n.m.chat.errors.switchBackend}</button>
          <button onclick={unloadModelMemory} disabled={conversationStore.loading || unloadingModel}>
            {i18n.m.chat.errors.unloadModel}
          </button>
          <button onclick={openLogsFromError}>{i18n.m.chat.errors.openLogs}</button>
        </div>
        <button
          class="error-dismiss"
          onclick={() => conversationStore.setLastError(null)}
          aria-label="Zavřít"
        >×</button>
      </div>
    {/if}

    {#if editImageStore.pending}
      <div class="edit-image-badge">
        <img src={convertFileSrc(editImageStore.pending)} alt="" class="edit-image-thumb" />
        <span class="edit-image-label">
          🎨 {i18n.t("chat.editingImage", { name: fileNameFromPath(editImageStore.pending) })}
        </span>
        <button
          class="edit-image-cancel"
          onclick={() => editImageStore.clear()}
          aria-label={i18n.m.chat.editCancel}
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

    {#if imagePromptAssistVisible}
      <div class="image-prompt-assist">
        <label class="translate-toggle">
          <input type="checkbox" bind:checked={translateImagePrompt} />
          <span>{i18n.m.chat.imagePrompt.translateToEnglish}</span>
        </label>
        {#if refImages.length > 0}
          <textarea
            class="reference-preservation-input"
            bind:value={referencePreservation}
            placeholder={i18n.m.chat.imagePrompt.referencePreservationPlaceholder}
            rows="2"
            disabled={conversationStore.loading}
          ></textarea>
        {/if}
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

    {#if showSubjects}
      <div class="subjects-popup" role="listbox">
        {#if subjectsStore.subjects.length === 0}
          <div class="subjects-empty">{i18n.m.subjects.emptyShort}</div>
        {/if}
        {#each subjectsStore.subjects as subject (subject.id)}
          <button
            class="subject-item"
            disabled={subject.images.length === 0}
            onclick={() => attachSubject(subject)}
          >
            {#if subject.images[0]}
              <img class="subject-thumb" src={convertFileSrc(subject.images[0].path)} alt="" />
            {/if}
            <span class="subject-name">{subject.name}</span>
            <span class="subject-count">{subject.images.length} 🖼</span>
          </button>
        {/each}
        <button
          class="subject-manage"
          onclick={() => { showSubjects = false; invoke("open_subjects_window").catch((e) => console.warn(e)); }}
        >⚙ {i18n.m.subjects.manage}</button>
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
      <button
        class="attach-btn"
        onclick={() => (showSubjects = !showSubjects)}
        class:active={showSubjects}
        disabled={conversationStore.loading}
        title={i18n.m.subjects.pickTitle}
        aria-label={i18n.m.subjects.pickTitle}
      >👤</button>
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

  .runtime-cluster {
    display: inline-flex;
    align-items: center;
    gap: 0.45rem;
  }

  .runtime-badge {
    border: 1px solid var(--color-border);
    border-radius: 999px;
    color: var(--color-text-muted);
    font-size: 0.72rem;
    font-weight: 700;
    padding: 0.16rem 0.5rem;
    white-space: nowrap;
  }
  .runtime-badge.gpu {
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 55%, var(--color-border));
    color: var(--color-success);
  }
  .runtime-badge.ram {
    background: color-mix(in srgb, var(--color-accent) 10%, transparent);
    border-color: color-mix(in srgb, var(--color-accent) 55%, var(--color-border));
    color: var(--color-accent);
  }
  .runtime-badge.npu {
    background: color-mix(in srgb, #0ea5e9 12%, transparent);
    border-color: color-mix(in srgb, #0ea5e9 55%, var(--color-border));
    color: #0284c7;
  }

  .unload-notice {
    color: var(--color-success);
    font-size: 0.76rem;
    font-weight: 600;
    white-space: nowrap;
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
  .gen-settings-btn:disabled {
    cursor: default;
    opacity: 0.45;
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

  .runtime-select {
    width: 100%;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    color: var(--color-text);
    font: inherit;
    padding: 0.45rem 0.6rem;
  }
  .runtime-select:focus {
    border-color: var(--color-accent);
    outline: none;
  }

  .gen-section {
    flex: 1 1 100%;
    font-size: 0.72rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
    border-top: 1px solid var(--color-border);
    padding-top: 0.6rem;
    margin-top: -0.25rem;
  }

  .gen-hint {
    margin: 0.3rem 0 0;
    font-size: 0.7rem;
    line-height: 1.3;
    color: var(--color-text-muted);
  }

  .gen-toggle {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.82rem;
    cursor: pointer;
  }

  .gen-toggle input[type="checkbox"] {
    accent-color: var(--color-accent);
    width: 1rem;
    height: 1rem;
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

  .bubble.image-progress {
    min-width: 320px;
    max-width: 75%;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
  }

  .image-progress-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 1rem;
  }

  .image-progress-label {
    font-size: 0.85rem;
    font-weight: 600;
  }

  .image-progress-elapsed {
    font-size: 0.75rem;
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }

  .image-progress-bar {
    height: 6px;
    background: var(--color-surface-2);
    border-radius: 3px;
    overflow: hidden;
    position: relative;
  }

  /* Neurčitý průběh — skutečná % u sampleru ComfyUI nehlásí */
  .image-progress-fill {
    position: absolute;
    height: 100%;
    width: 35%;
    background: var(--color-accent);
    border-radius: 3px;
    animation: indeterminate 1.4s ease-in-out infinite;
  }

  @keyframes indeterminate {
    0% { left: -35%; }
    100% { left: 100%; }
  }

  /* Skutečná procenta (kroky sampleru) — bez animace, plynulá šířka */
  .image-progress-fill.determinate {
    position: static;
    animation: none;
    transition: width 0.3s ease;
  }

  .image-progress-detail {
    font-size: 0.72rem;
    color: var(--color-text-muted);
    font-family: "JetBrains Mono", monospace;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 480px;
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
    flex: 1 1 auto;
    line-height: 1.5;
    word-break: break-word;
  }

  .error-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    justify-content: flex-end;
  }

  .error-actions button {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 7px;
    color: var(--color-text);
    cursor: pointer;
    font-size: 0.75rem;
    padding: 0.25rem 0.45rem;
    white-space: nowrap;
  }

  .error-actions button:hover:not(:disabled) {
    border-color: var(--color-accent);
  }

  .error-actions button:disabled {
    cursor: default;
    opacity: 0.45;
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

  .edit-image-badge {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    margin: 0.75rem 1.25rem 0;
    padding: 0.4rem 0.6rem;
    background: var(--color-user-bubble);
    border: 1px solid var(--color-accent);
    border-radius: 10px;
    font-size: 0.82rem;
  }

  .edit-image-thumb {
    width: 40px;
    height: 40px;
    object-fit: cover;
    border-radius: 6px;
    border: 1px solid var(--color-border);
  }

  .edit-image-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .edit-image-cancel {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 1rem;
    line-height: 1;
    padding: 0 0.15rem;
  }
  .edit-image-cancel:hover { color: var(--color-text); }

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

  .image-prompt-assist {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    margin: 0.6rem 1.25rem 0;
    padding: 0.55rem 0.65rem;
    background: color-mix(in srgb, var(--color-accent) 7%, transparent);
    border: 1px solid var(--color-border);
    border-radius: 10px;
  }

  .translate-toggle {
    display: flex;
    align-items: center;
    gap: 0.45rem;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 0.78rem;
  }

  .translate-toggle input {
    accent-color: var(--color-accent);
  }

  .reference-preservation-input {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    min-height: 44px;
    max-height: 110px;
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.45rem 0.55rem;
    font: inherit;
    font-size: 0.8rem;
    line-height: 1.35;
  }

  .reference-preservation-input:focus {
    outline: none;
    border-color: var(--color-accent);
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

  .subjects-popup {
    position: absolute;
    bottom: 100%;
    left: 1.25rem;
    margin-bottom: 0.4rem;
    min-width: 240px;
    max-height: 300px;
    overflow-y: auto;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.3);
    z-index: 30;
    padding: 0.3rem;
  }
  .subjects-empty {
    padding: 0.6rem 0.7rem;
    font-size: 0.8rem;
    color: var(--color-text-muted);
  }
  .subject-item {
    display: flex;
    align-items: center;
    gap: 0.55rem;
    width: 100%;
    background: transparent;
    border: none;
    padding: 0.4rem 0.5rem;
    border-radius: 8px;
    cursor: pointer;
    color: var(--color-text);
  }
  .subject-item:hover:not(:disabled) {
    background: var(--color-surface-2);
  }
  .subject-item:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .subject-thumb {
    width: 28px;
    height: 28px;
    border-radius: 6px;
    object-fit: cover;
    flex-shrink: 0;
  }
  .subject-name {
    flex: 1;
    text-align: left;
    font-size: 0.85rem;
    font-weight: 600;
  }
  .subject-count {
    font-size: 0.72rem;
    color: var(--color-text-muted);
  }
  .subject-manage {
    display: block;
    width: 100%;
    text-align: left;
    background: transparent;
    border: none;
    border-top: 1px solid var(--color-border);
    margin-top: 0.3rem;
    padding: 0.5rem;
    font-size: 0.8rem;
    color: var(--color-text-muted);
    cursor: pointer;
  }
  .subject-manage:hover {
    color: var(--color-accent);
  }
  .attach-btn.active {
    border-color: var(--color-accent);
    color: var(--color-accent);
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
