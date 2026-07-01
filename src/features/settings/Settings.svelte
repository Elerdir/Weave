<script lang="ts">
  import { onMount } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { Locale } from "$lib/i18n/index.svelte";
  import { themeStore } from "$lib/theme/index.svelte";
  import type { Theme } from "$lib/theme/index.svelte";
  import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import type { ApiServiceId } from "$lib/stores/settings.svelte";
  import { modelsStore, formatBytes } from "$lib/stores/models.svelte";

  async function pickModelFile() {
    const path = await openFilePicker({
      multiple: false,
      filters: [{ name: "GGUF model", extensions: ["gguf"] }],
    });
    if (typeof path === "string") {
      settingsStore.setModelPath(path);
      await settingsStore.saveModelPath();
    }
  }

  let { onClose }: { onClose: () => void } = $props();

  type Section = "appearance" | "language" | "apiKeys" | "llm" | "comfyui" | "models" | "notifications";
  let section = $state<Section>("appearance");

  let downloadUrl = $state("");
  let downloadId = $state("");

  const backendLabel: Record<string, string> = {
    cuda: "CUDA (NVIDIA)",
    metal: "Metal (Apple)",
    vulkan: "Vulkan",
    cpu: "CPU",
  };

  // Dočasné hodnoty pro zadání nových klíčů
  let keyInputs = $state<Record<ApiServiceId, string>>({
    mistral: "",
    civitai: "",
    huggingface: "",
  });

  const themes: Theme[] = ["light", "dark", "system"];
  const locales: { value: Locale; label: string }[] = [
    { value: "cs", label: "Čeština" },
    { value: "en", label: "English" },
  ];
  const services: { id: ApiServiceId; label: string }[] = [
    { id: "mistral", label: "Mistral" },
    { id: "civitai", label: "CivitAI" },
    { id: "huggingface", label: "HuggingFace" },
  ];

  onMount(() => {
    settingsStore.load().catch((e) => console.warn("settings load selhal:", e));
    modelsStore.load().catch((e) => console.warn("models load selhal:", e));
  });

  async function startDownload() {
    if (!downloadId.trim() || !downloadUrl.trim()) return;
    await modelsStore.downloadModel(downloadId.trim(), downloadUrl.trim());
    downloadId = "";
    downloadUrl = "";
  }

  function downloadPercent(): number {
    const d = modelsStore.download;
    if (!d || d.total === 0) return 0;
    return Math.round((d.downloaded / d.total) * 100);
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }

  async function saveKey(service: ApiServiceId) {
    const token = keyInputs[service];
    if (!token.trim()) return;
    await settingsStore.saveKey(service, token);
    keyInputs[service] = "";
  }

  function themeLabel(t: Theme): string {
    return i18n.m.settings.theme[t];
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="settings-overlay">
  <div class="settings-card">
    <header class="settings-header">
      <h2>{i18n.m.settings.title}</h2>
      <button class="btn-close" onclick={onClose} aria-label="Zavřít">✕</button>
    </header>

    <div class="settings-body">
      <nav class="settings-nav">
        <button class:active={section === "appearance"} onclick={() => (section = "appearance")}>
          {i18n.m.settings.sections.appearance}
        </button>
        <button class:active={section === "language"} onclick={() => (section = "language")}>
          {i18n.m.settings.sections.language}
        </button>
        <button class:active={section === "apiKeys"} onclick={() => (section = "apiKeys")}>
          {i18n.m.settings.sections.apiKeys}
        </button>
        <button class:active={section === "llm"} onclick={() => (section = "llm")}>
          {i18n.m.settings.sections.llm}
        </button>
        <button class:active={section === "comfyui"} onclick={() => (section = "comfyui")}>
          {i18n.m.settings.sections.comfyui}
        </button>
        <button class:active={section === "models"} onclick={() => (section = "models")}>
          {i18n.m.settings.sections.models}
        </button>
        <button class:active={section === "notifications"} onclick={() => (section = "notifications")}>
          {i18n.m.settings.sections.notifications}
        </button>
      </nav>

      <div class="settings-content">
        {#if section === "appearance"}
          <h3>{i18n.m.settings.theme.label}</h3>
          <div class="option-row">
            {#each themes as t}
              <button
                class="chip"
                class:selected={themeStore.theme === t}
                onclick={() => themeStore.setTheme(t)}
              >
                {themeLabel(t)}
              </button>
            {/each}
          </div>
        {:else if section === "language"}
          <h3>{i18n.m.settings.sections.language}</h3>
          <div class="option-row">
            {#each locales as loc}
              <button
                class="chip"
                class:selected={i18n.locale === loc.value}
                onclick={() => i18n.setLocale(loc.value)}
              >
                {loc.label}
              </button>
            {/each}
          </div>
        {:else if section === "apiKeys"}
          <h3>{i18n.m.settings.sections.apiKeys}</h3>
          <p class="hint">{i18n.m.settings.apiKeys.masked}</p>
          <div class="keys">
            {#each services as svc}
              {@const state = settingsStore.apiKeys[svc.id]}
              <div class="key-row">
                <div class="key-head">
                  <span class="key-name">{svc.label}</span>
                  {#if state.hasKey}
                    <span class="key-status set">{state.masked ?? i18n.m.settings.apiKeys.stored}</span>
                  {:else}
                    <span class="key-status unset">{i18n.m.settings.apiKeys.notStored}</span>
                  {/if}
                </div>
                <div class="key-actions">
                  <input
                    type="password"
                    placeholder={i18n.m.wizard.steps.apiKeys.placeholder}
                    bind:value={keyInputs[svc.id]}
                    onkeydown={(e) => e.key === "Enter" && saveKey(svc.id)}
                  />
                  <button class="btn-sm primary" onclick={() => saveKey(svc.id)} disabled={!keyInputs[svc.id].trim()}>
                    {i18n.m.settings.apiKeys.update}
                  </button>
                  {#if state.hasKey}
                    <button class="btn-sm danger" onclick={() => settingsStore.deleteKey(svc.id)}>
                      {i18n.m.settings.apiKeys.delete}
                    </button>
                  {/if}
                </div>
              </div>
            {/each}
          </div>
        {:else if section === "llm"}
          <h3>{i18n.m.settings.llm.backend}</h3>
          <p class="hint">{i18n.m.settings.llm.hint}</p>
          <div class="option-row">
            <button
              class="chip"
              class:selected={settingsStore.llmBackend === "mistral"}
              onclick={() => settingsStore.setBackend("mistral")}
            >
              {i18n.m.settings.llm.mistral}
            </button>
            <button
              class="chip"
              class:selected={settingsStore.llmBackend === "local"}
              onclick={() => settingsStore.setBackend("local")}
            >
              {i18n.m.settings.llm.local}
            </button>
            <button
              class="chip"
              class:selected={settingsStore.llmBackend === "embedded"}
              onclick={() => settingsStore.setBackend("embedded")}
            >
              {i18n.m.settings.llm.embedded}
            </button>
          </div>

          {#if settingsStore.llmBackend === "embedded"}
            <p class="hint" style="margin-top:1rem">{i18n.m.settings.llm.embeddedHint}</p>
            <label class="field-label" for="model-path">{i18n.m.settings.llm.modelPath}</label>
            <div class="comfyui-row">
              <input
                id="model-path"
                type="text"
                readonly
                value={settingsStore.modelPath}
                placeholder={i18n.m.settings.llm.modelPathPlaceholder}
              />
              <button class="btn-sm primary" onclick={pickModelFile}>
                {i18n.m.settings.llm.browse}
              </button>
            </div>
            <label class="field-label" for="gpu-layers" style="margin-top:0.75rem">
              {i18n.m.settings.llm.gpuLayers}
            </label>
            <input
              id="gpu-layers"
              class="gpu-layers-input"
              type="number"
              min="0"
              value={settingsStore.gpuLayers}
              oninput={(e) => settingsStore.setGpuLayers((e.target as HTMLInputElement).value)}
              onblur={() => settingsStore.saveGpuLayers()}
            />
          {/if}

          {#if settingsStore.llmBackend === "local"}
            <label class="field-label" for="local-url" style="margin-top:1rem">
              {i18n.m.settings.llm.localUrl}
            </label>
            <div class="comfyui-row">
              <input
                id="local-url"
                type="text"
                value={settingsStore.localUrl}
                oninput={(e) => settingsStore.setLocalUrl((e.target as HTMLInputElement).value)}
                onblur={() => settingsStore.saveLocalUrl()}
              />
              <button class="btn-sm primary" onclick={() => settingsStore.testLocal()}>
                {i18n.m.settings.llm.test}
              </button>
            </div>
            {#if settingsStore.localStatus === "connected"}
              <span class="conn-status connected">● {i18n.m.settings.llm.connected}</span>
            {:else if settingsStore.localStatus === "disconnected"}
              <span class="conn-status disconnected">● {i18n.m.settings.llm.disconnected}</span>
            {:else if settingsStore.localStatus === "testing"}
              <span class="conn-status testing">{i18n.m.common.loading}</span>
            {/if}
          {/if}
        {:else if section === "comfyui"}
          <h3>{i18n.m.settings.sections.comfyui}</h3>
          <label class="field-label" for="comfyui-url">{i18n.m.settings.comfyui.url}</label>
          <div class="comfyui-row">
            <input
              id="comfyui-url"
              type="text"
              value={settingsStore.comfyuiUrl}
              oninput={(e) => settingsStore.setComfyuiUrl((e.target as HTMLInputElement).value)}
              onblur={() => settingsStore.saveComfyuiUrl()}
            />
            <button class="btn-sm primary" onclick={() => settingsStore.testComfyui()}>
              {i18n.m.settings.comfyui.test}
            </button>
          </div>
          {#if settingsStore.comfyuiStatus === "connected"}
            <span class="conn-status connected">● {i18n.m.settings.comfyui.connected}</span>
          {:else if settingsStore.comfyuiStatus === "disconnected"}
            <span class="conn-status disconnected">● {i18n.m.settings.comfyui.disconnected}</span>
          {:else if settingsStore.comfyuiStatus === "testing"}
            <span class="conn-status testing">{i18n.m.common.loading}</span>
          {/if}
        {:else if section === "models"}
          <h3>{i18n.m.settings.models.title}</h3>

          {#if modelsStore.gpu}
            <div class="gpu-info">
              <span class="gpu-icon">⚡</span>
              <span>{modelsStore.gpu.name}</span>
              <span class="gpu-backend">{backendLabel[modelsStore.gpu.backend] ?? modelsStore.gpu.backend}</span>
              {#if modelsStore.gpu.vram_mb > 0}
                <span class="gpu-vram">{Math.round(modelsStore.gpu.vram_mb / 1024)} GB VRAM</span>
              {/if}
            </div>
          {/if}

          <div class="model-list">
            {#each modelsStore.models as model (model.id)}
              <div class="model-item">
                <div class="model-meta">
                  <span class="model-name">{model.name}</span>
                  <span class="model-size">{formatBytes(model.size_bytes)}</span>
                </div>
                <button class="btn-sm danger" onclick={() => modelsStore.deleteModel(model.id)}>
                  {i18n.m.settings.models.delete}
                </button>
              </div>
            {/each}

            {#if modelsStore.models.length === 0 && !modelsStore.download}
              <p class="hint">{i18n.m.settings.models.noModels}</p>
            {/if}
          </div>

          {#if modelsStore.download}
            <div class="dl-progress">
              <div class="dl-head">
                <span>{modelsStore.download.modelId}</span>
                <span>
                  {#if modelsStore.download.phase === "verifying"}
                    {i18n.m.common.loading}
                  {:else}
                    {downloadPercent()}% · {formatBytes(modelsStore.download.downloaded)} / {formatBytes(modelsStore.download.total)}
                  {/if}
                </span>
              </div>
              <div class="dl-bar">
                <div class="dl-fill" style="width: {downloadPercent()}%"></div>
              </div>
            </div>
          {:else}
            <div class="dl-form">
              <input
                type="text"
                placeholder="ID modelu (např. mistral-7b)"
                bind:value={downloadId}
              />
              <input
                type="text"
                placeholder="URL ke stažení (HuggingFace / CivitAI)"
                bind:value={downloadUrl}
              />
              <button
                class="btn-sm primary"
                onclick={startDownload}
                disabled={!downloadId.trim() || !downloadUrl.trim()}
              >
                ↓
              </button>
            </div>
          {/if}

          {#if modelsStore.error}
            <span class="conn-status disconnected">{modelsStore.error}</span>
          {/if}
        {:else if section === "notifications"}
          <h3>{i18n.m.settings.notifications.label}</h3>
          <p class="hint">{i18n.m.settings.notifications.hint}</p>
          <div class="option-row">
            <button
              class="chip"
              class:selected={settingsStore.notificationsEnabled}
              onclick={() => settingsStore.setNotifications(true)}
            >
              {i18n.m.settings.notifications.enabled}
            </button>
            <button
              class="chip"
              class:selected={!settingsStore.notificationsEnabled}
              onclick={() => settingsStore.setNotifications(false)}
            >
              {i18n.m.settings.notifications.disabled}
            </button>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  .settings-overlay {
    position: fixed;
    inset: 0;
    z-index: 60;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .settings-card {
    width: 720px;
    max-width: calc(100vw - 2rem);
    height: 520px;
    max-height: calc(100vh - 2rem);
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 14px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .settings-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 1rem 1.25rem;
    border-bottom: 1px solid var(--color-border);
  }

  .settings-header h2 {
    font-size: 1.1rem;
    font-weight: 600;
  }

  .btn-close {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    font-size: 1rem;
    cursor: pointer;
    border-radius: 6px;
    padding: 0.25rem 0.5rem;
  }
  .btn-close:hover {
    color: var(--color-text);
    background: var(--color-surface-2);
  }

  .settings-body {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .settings-nav {
    width: 180px;
    border-right: 1px solid var(--color-border);
    padding: 0.75rem;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .settings-nav button {
    text-align: left;
    background: transparent;
    border: none;
    color: var(--color-text);
    padding: 0.55rem 0.75rem;
    border-radius: 8px;
    font-size: 0.875rem;
    cursor: pointer;
    transition: background 0.15s;
  }
  .settings-nav button:hover {
    background: var(--color-surface-2);
  }
  .settings-nav button.active {
    background: var(--color-user-bubble);
    color: var(--color-text);
    font-weight: 600;
  }

  .settings-content {
    flex: 1;
    padding: 1.25rem 1.5rem;
    overflow-y: auto;
  }

  .settings-content h3 {
    font-size: 0.95rem;
    font-weight: 600;
    margin-bottom: 1rem;
  }

  .hint {
    font-size: 0.8rem;
    color: var(--color-text-muted);
    margin-bottom: 1rem;
  }

  .option-row {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }

  .chip {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.5rem 1rem;
    font-size: 0.875rem;
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
  }
  .chip:hover {
    border-color: var(--color-text-muted);
  }
  .chip.selected {
    border-color: var(--color-accent);
    background: var(--color-user-bubble);
  }

  .keys {
    display: flex;
    flex-direction: column;
    gap: 1rem;
  }

  .key-row {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .key-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }

  .key-name {
    font-weight: 600;
    font-size: 0.875rem;
  }

  .key-status {
    font-size: 0.78rem;
    font-family: monospace;
  }
  .key-status.set {
    color: var(--color-success);
  }
  .key-status.unset {
    color: var(--color-text-muted);
  }

  .key-actions {
    display: flex;
    gap: 0.5rem;
  }

  .key-actions input,
  .comfyui-row input {
    flex: 1;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 7px;
    padding: 0.45rem 0.65rem;
    font-size: 0.85rem;
    outline: none;
  }
  .key-actions input:focus,
  .comfyui-row input:focus {
    border-color: var(--color-accent);
  }

  .btn-sm {
    border: none;
    border-radius: 7px;
    padding: 0.45rem 0.85rem;
    font-size: 0.8rem;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .btn-sm.primary {
    background: var(--color-accent);
    color: #fff;
  }
  .btn-sm.primary:hover:not(:disabled) {
    background: var(--color-accent-hover);
  }
  .btn-sm.primary:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .btn-sm.danger {
    background: transparent;
    color: var(--color-error);
    border: 1px solid var(--color-border);
  }
  .btn-sm.danger:hover {
    border-color: var(--color-error);
  }

  .field-label {
    display: block;
    font-size: 0.8rem;
    color: var(--color-text-muted);
    margin-bottom: 0.4rem;
  }

  .comfyui-row {
    display: flex;
    gap: 0.5rem;
  }

  .gpu-layers-input {
    width: 100px;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 7px;
    padding: 0.45rem 0.65rem;
    font-size: 0.85rem;
    outline: none;
  }
  .gpu-layers-input:focus {
    border-color: var(--color-accent);
  }

  .conn-status {
    display: inline-block;
    margin-top: 0.75rem;
    font-size: 0.82rem;
  }
  .conn-status.connected {
    color: var(--color-success);
  }
  .conn-status.disconnected {
    color: var(--color-error);
  }
  .conn-status.testing {
    color: var(--color-text-muted);
  }

  .gpu-info {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.6rem 0.85rem;
    font-size: 0.82rem;
    margin-bottom: 1rem;
  }
  .gpu-icon {
    font-size: 1rem;
  }
  .gpu-backend,
  .gpu-vram {
    color: var(--color-text-muted);
    font-size: 0.78rem;
  }
  .gpu-vram {
    margin-left: auto;
  }

  .model-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-bottom: 1rem;
  }

  .model-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.55rem 0.85rem;
  }

  .model-meta {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }
  .model-name {
    font-size: 0.875rem;
    font-weight: 600;
  }
  .model-size {
    font-size: 0.75rem;
    color: var(--color-text-muted);
  }

  .dl-form {
    display: flex;
    gap: 0.5rem;
  }
  .dl-form input {
    flex: 1;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 7px;
    padding: 0.45rem 0.65rem;
    font-size: 0.82rem;
    outline: none;
  }
  .dl-form input:focus {
    border-color: var(--color-accent);
  }

  .dl-progress {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }
  .dl-head {
    display: flex;
    justify-content: space-between;
    font-size: 0.8rem;
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }
  .dl-bar {
    height: 6px;
    background: var(--color-border);
    border-radius: 3px;
    overflow: hidden;
  }
  .dl-fill {
    height: 100%;
    background: var(--color-accent);
    border-radius: 3px;
    transition: width 0.2s;
  }
</style>
