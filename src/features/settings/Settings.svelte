<script lang="ts">
  import { onMount } from "svelte";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { Locale } from "$lib/i18n/index.svelte";
  import { themeStore } from "$lib/theme/index.svelte";
  import type { Theme } from "$lib/theme/index.svelte";
  import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
  import { open as openUrl } from "@tauri-apps/plugin-shell";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import type { ApiServiceId } from "$lib/stores/settings.svelte";
  import { modelsStore, formatBytes } from "$lib/stores/models.svelte";
  import { comfyInstallStore } from "$lib/stores/comfy-install.svelte";
  import { TOKEN_URLS } from "$lib/token-urls";
  import LogViewer from "./LogViewer.svelte";

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

  async function openTokenPage(service: ApiServiceId) {
    try {
      await openUrl(TOKEN_URLS[service]);
    } catch (e) {
      console.warn("Nepodařilo se otevřít odkaz:", e);
    }
  }

  let { onClose }: { onClose: () => void } = $props();

  type Section = "appearance" | "language" | "apiKeys" | "llm" | "comfyui" | "models" | "notifications" | "logs";
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
    comfyInstallStore.load().catch((e) => console.warn("comfy status load selhal:", e));
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
        <button class:active={section === "logs"} onclick={() => (section = "logs")}>
          {i18n.m.settings.sections.logs}
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
                  <span class="key-name">
                    {svc.label}
                    <button
                      type="button"
                      class="link-btn"
                      onclick={() => openTokenPage(svc.id)}
                      title={i18n.m.wizard.steps.apiKeys.howToGet}
                      aria-label={i18n.m.wizard.steps.apiKeys.howToGet}
                    >🌐</button>
                  </span>
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

            {#if settingsStore.modelPath}
              <div class="active-model">
                <span class="active-model-label">{i18n.m.settings.llm.activeModel}</span>
                <span class="active-model-path">{settingsStore.modelPath.split(/[\\/]/).pop()}</span>
              </div>
            {/if}

            <h4 class="sub-heading">{i18n.m.settings.llm.recommendedTitle}</h4>
            <div class="recommended-list">
              {#each modelsStore.recommended as rec (rec.id)}
                {@const downloaded = modelsStore.isDownloaded(rec.id)}
                {@const isActive = settingsStore.modelPath.endsWith(`${rec.id}.gguf`)}
                <div class="recommended-item" class:active={isActive}>
                  <div class="recommended-meta">
                    <div class="recommended-name">
                      {rec.name}
                      {#if isActive}<span class="active-badge">{i18n.m.settings.llm.inUse}</span>{/if}
                    </div>
                    <div class="recommended-desc">{rec.description}</div>
                    <div class="recommended-size">{formatBytes(rec.size_bytes)}</div>
                  </div>
                  {#if downloaded}
                    <div class="recommended-actions">
                      <button
                        class="btn-sm"
                        class:primary={!isActive}
                        disabled={isActive}
                        onclick={() => {
                          settingsStore.setModelPath(modelsStore.models.find((m) => m.id === rec.id)?.path ?? "");
                          settingsStore.saveModelPath();
                        }}
                      >
                        {isActive ? i18n.m.settings.llm.inUse : i18n.m.settings.llm.activate}
                      </button>
                      <button
                        class="btn-sm danger"
                        disabled={isActive}
                        title={i18n.m.settings.models.delete}
                        aria-label={i18n.m.settings.models.delete}
                        onclick={() => modelsStore.deleteModel(rec.id)}
                      >🗑</button>
                    </div>
                  {:else if modelsStore.download?.modelId === rec.id}
                    <span class="dl-inline">
                      {Math.round(((modelsStore.download.downloaded) / (modelsStore.download.total || 1)) * 100)}%
                    </span>
                  {:else}
                    <button
                      class="btn-sm primary"
                      disabled={!!modelsStore.download}
                      onclick={() => modelsStore.downloadRecommended(rec.id)}
                    >
                      {i18n.m.settings.llm.download}
                    </button>
                  {/if}
                </div>
              {/each}
            </div>

            {#if modelsStore.download}
              <div class="dl-progress" style="margin-top:0.75rem">
                <div class="dl-head">
                  <span>{modelsStore.download.modelId}</span>
                  <span>
                    {#if modelsStore.download.phase === "verifying"}
                      {i18n.m.common.loading}
                    {:else}
                      {formatBytes(modelsStore.download.downloaded)} / {formatBytes(modelsStore.download.total)}
                    {/if}
                  </span>
                </div>
                <div class="dl-bar">
                  <div
                    class="dl-fill"
                    style="width: {Math.round((modelsStore.download.downloaded / (modelsStore.download.total || 1)) * 100)}%"
                  ></div>
                </div>
              </div>
            {/if}

            {#if modelsStore.error}
              <span class="conn-status disconnected">{modelsStore.error}</span>
            {/if}

            <details class="advanced-details">
              <summary>{i18n.m.settings.llm.advanced}</summary>
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
              <label class="field-label" for="context-length" style="margin-top:0.75rem">
                {i18n.m.settings.llm.contextLength}
              </label>
              <input
                id="context-length"
                class="gpu-layers-input"
                type="number"
                min="512"
                step="1024"
                value={settingsStore.contextLength}
                oninput={(e) =>
                  settingsStore.setContextLength((e.target as HTMLInputElement).value)}
                onblur={() => settingsStore.saveContextLength()}
              />
            </details>
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

          <h4 class="sub-heading">{i18n.m.settings.comfyui.localTitle}</h4>
          <p class="hint">{i18n.m.settings.comfyui.localHint}</p>

          {#if comfyInstallStore.status === "NotInstalled" && !comfyInstallStore.installing}
            <button class="btn-sm primary" onclick={() => comfyInstallStore.install()}>
              {i18n.m.settings.comfyui.installButton}
            </button>
          {:else if comfyInstallStore.installing}
            <div class="install-progress">
              <div class="install-step">
                <span class="spinner"></span>
                {comfyInstallStore.currentStep || i18n.m.common.loading}
              </div>
              <pre class="install-log">{comfyInstallStore.log.join("\n")}</pre>
            </div>
          {:else if comfyInstallStore.status === "Installed"}
            <div class="comfy-status-row">
              <span class="conn-status connected">● {i18n.m.settings.comfyui.installedLabel}</span>
              <button
                class="btn-sm primary"
                disabled={comfyInstallStore.starting}
                onclick={() => comfyInstallStore.startServer()}
              >
                {comfyInstallStore.starting ? i18n.m.common.loading : i18n.m.settings.comfyui.startServer}
              </button>
            </div>
          {:else if comfyInstallStore.status === "Running"}
            <div class="comfy-status-row">
              <span class="conn-status connected">● {i18n.m.settings.comfyui.runningLabel}</span>
              <button class="btn-sm danger" onclick={() => comfyInstallStore.stopServer()}>
                {i18n.m.settings.comfyui.stopServer}
              </button>
            </div>
          {/if}

          {#if comfyInstallStore.error}
            <span class="conn-status disconnected">{comfyInstallStore.error}</span>
          {/if}

          {#if comfyInstallStore.checkpoints.length > 0}
            <h4 class="sub-heading">{i18n.m.settings.comfyui.imageModelsTitle}</h4>
            <div class="recommended-list">
              {#each comfyInstallStore.checkpoints as ckpt (ckpt.file_name)}
                <div class="recommended-item">
                  <div class="recommended-meta">
                    <div class="recommended-name">{ckpt.file_name}</div>
                    <div class="recommended-size">{formatBytes(ckpt.size_bytes)}</div>
                  </div>
                  <button
                    class="btn-sm danger"
                    title={i18n.m.settings.models.delete}
                    aria-label={i18n.m.settings.models.delete}
                    onclick={() => comfyInstallStore.deleteCheckpoint(ckpt.file_name)}
                  >🗑</button>
                </div>
              {/each}
            </div>
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
        {:else if section === "logs"}
          <h3>{i18n.m.settings.sections.logs}</h3>
          <LogViewer />
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
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }

  .link-btn {
    background: transparent;
    border: none;
    cursor: pointer;
    font-size: 0.85rem;
    padding: 0.1rem 0.25rem;
    border-radius: 5px;
    opacity: 0.7;
    transition: opacity 0.15s, background 0.15s;
  }
  .link-btn:hover {
    opacity: 1;
    background: var(--color-surface-2);
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
  .recommended-actions {
    display: flex;
    gap: 0.4rem;
    align-items: center;
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

  .active-model {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    background: var(--color-surface-2);
    border: 1px solid var(--color-accent);
    border-radius: 8px;
    padding: 0.5rem 0.75rem;
    font-size: 0.82rem;
    margin: 0.75rem 0;
  }
  .active-model-label {
    color: var(--color-text-muted);
  }
  .active-model-path {
    font-family: monospace;
    font-weight: 600;
  }

  .sub-heading {
    font-size: 0.82rem;
    font-weight: 600;
    color: var(--color-text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
    margin: 1rem 0 0.5rem;
  }

  .recommended-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .recommended-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.65rem 0.85rem;
  }
  .recommended-item.active {
    border-color: var(--color-accent);
  }

  .recommended-name {
    font-size: 0.875rem;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .active-badge {
    font-size: 0.68rem;
    font-weight: 600;
    color: var(--color-accent);
    background: var(--color-user-bubble);
    border-radius: 4px;
    padding: 0.05rem 0.4rem;
  }
  .recommended-desc {
    font-size: 0.78rem;
    color: var(--color-text-muted);
    margin-top: 0.15rem;
    max-width: 32rem;
  }
  .recommended-size {
    font-size: 0.72rem;
    color: var(--color-text-muted);
    margin-top: 0.25rem;
  }

  .dl-inline {
    font-size: 0.8rem;
    color: var(--color-accent);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .advanced-details {
    margin-top: 1.25rem;
    border-top: 1px solid var(--color-border);
    padding-top: 0.75rem;
  }
  .advanced-details summary {
    cursor: pointer;
    font-size: 0.8rem;
    color: var(--color-text-muted);
  }
  .advanced-details summary:hover {
    color: var(--color-text);
  }
  .advanced-details .field-label {
    margin-top: 0.75rem;
  }

  .comfy-status-row {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.5rem;
  }

  .install-progress {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-top: 0.5rem;
  }

  .install-step {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    font-size: 0.875rem;
    font-weight: 600;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border: 2px solid var(--color-border);
    border-top-color: var(--color-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
    flex-shrink: 0;
  }
  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .install-log {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.6rem 0.75rem;
    font-family: "JetBrains Mono", "Fira Code", monospace;
    font-size: 0.72rem;
    line-height: 1.5;
    color: var(--color-text-muted);
    max-height: 220px;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-all;
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
