<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { i18n } from "$lib/i18n/index.svelte";
  import type { Locale } from "$lib/i18n/index.svelte";
  import { themeStore } from "$lib/theme/index.svelte";
  import type { Theme } from "$lib/theme/index.svelte";
  import { open as openFilePicker, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { open as openUrl } from "@tauri-apps/plugin-shell";
  import { settingsStore } from "$lib/stores/settings.svelte";
  import type { ApiServiceId } from "$lib/stores/settings.svelte";
  import { modelsStore, formatBytes, formatSpeed, formatEta } from "$lib/stores/models.svelte";
  import { modelSearchStore, modelIdForFile } from "$lib/stores/model-search.svelte";
  import { civitaiBrowserStore } from "$lib/stores/civitai-browser.svelte";
  import { comfyInstallStore } from "$lib/stores/comfy-install.svelte";
  import { openvinoInstallStore } from "$lib/stores/openvino-install.svelte";
  import { updaterStore } from "$lib/stores/updater.svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { TOKEN_URLS } from "$lib/token-urls";
  import LogViewer from "./LogViewer.svelte";

  async function pickModelFile() {
    const path = await openFilePicker({
      multiple: false,
      filters: [{ name: "GGUF model", extensions: ["gguf"] }],
    });
    if (typeof path === "string") {
      await settingsStore.activateModel(path);
    }
  }

  let unloadingVram = $state(false);
  let vramUnloaded = $state(false);
  let restartingRuntime = $state(false);
  let runtimeRestartNotice = $state<string | null>(null);

  async function unloadVram() {
    unloadingVram = true;
    vramUnloaded = false;
    try {
      await settingsStore.unloadEmbeddedModel();
      vramUnloaded = true;
    } finally {
      unloadingVram = false;
    }
  }

  async function restartRuntime() {
    if (restartingRuntime) return;
    restartingRuntime = true;
    runtimeRestartNotice = null;
    try {
      await invoke("restart_runtime", {
        openvinoModelDir: openvinoInstallStore.status?.serverRunning
          ? openvinoInstallStore.modelDir.trim()
          : null,
      });
      runtimeRestartNotice = i18n.m.settings.runtime.restartDone;
      await refreshRuntime();
    } catch (e) {
      runtimeRestartNotice = String(e);
    } finally {
      restartingRuntime = false;
    }
  }

  // Bez try/catch skoncila chyba dialogu jako neodchycene odmitnuti promise
  // a tlacitko navenek "nic nedelalo" — uzivatel nemel jak zjistit proc.
  async function pickModelsDir() {
    try {
      const dir = await openFilePicker({ directory: true, multiple: false });
      if (typeof dir === "string") {
        await modelsStore.setModelsDir(dir);
      }
    } catch (e) {
      modelsStore.setError(`Nepodařilo se otevřít výběr složky: ${e}`);
    }
  }

  async function pickOpenvinoModelDir() {
    try {
      const dir = await openFilePicker({ directory: true, multiple: false });
      if (typeof dir === "string") {
        openvinoInstallStore.setModelDir(dir);
      }
    } catch (e) {
      modelsStore.setError(`Nepodařilo se otevřít výběr složky: ${e}`);
    }
  }

  async function uninstallComfyui() {
    if (!confirm(i18n.m.settings.comfyui.uninstallConfirm)) return;
    await comfyInstallStore.uninstall();
  }

  function diagStatus(ok: boolean) {
    return ok ? i18n.m.settings.comfyui.diagOk : i18n.m.settings.comfyui.diagMissing;
  }

  async function openTokenPage(service: ApiServiceId) {
    try {
      await openUrl(TOKEN_URLS[service]);
    } catch (e) {
      console.warn("Nepodařilo se otevřít odkaz:", e);
    }
  }

  async function openSelectedOpenvinoSource() {
    const sourceUrl = openvinoInstallStore.selectedProfile?.sourceUrl;
    if (!sourceUrl) return;
    try {
      await openUrl(sourceUrl);
    } catch (e) {
      console.warn("Nepodarilo se otevrit OpenVINO model zdroj:", e);
    }
  }

  let { onClose, windowMode = false }: { onClose: () => void; windowMode?: boolean } = $props();

  type Section = "appearance" | "language" | "apiKeys" | "llm" | "runtime" | "downloads" | "comfyui" | "models" | "notifications" | "logs" | "updates" | "backup";
  // Záloha dat: export/import ZIP (import se aplikuje po restartu)
  let backupBusy = $state(false);
  let backupNotice = $state<string | null>(null);
  let backupError = $state<string | null>(null);
  let restorePending = $state(false);

  async function exportBackup() {
    backupError = null;
    backupNotice = null;
    const date = new Date().toISOString().slice(0, 10);
    const dest = await saveFileDialog({
      defaultPath: `weave-zaloha-${date}.zip`,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (!dest) return;
    backupBusy = true;
    try {
      const size = await invoke<number>("export_backup", { dest });
      backupNotice = i18n.t("settings.backup.exportDone", {
        size: formatBytes(size),
      });
    } catch (e) {
      backupError = String(e);
    } finally {
      backupBusy = false;
    }
  }

  async function importBackup() {
    backupError = null;
    backupNotice = null;
    const src = await openFilePicker({
      multiple: false,
      filters: [{ name: "ZIP", extensions: ["zip"] }],
    });
    if (!src || Array.isArray(src)) return;
    if (!confirm(i18n.m.settings.backup.importConfirm)) return;
    backupBusy = true;
    try {
      await invoke("import_backup", { src });
      restorePending = true;
    } catch (e) {
      backupError = String(e);
    } finally {
      backupBusy = false;
    }
  }

  let section = $state<Section>("appearance");

  let appVersion = $state("");

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

  function isRtx3090Model(rec: { id: string; size_bytes: number }) {
    return (
      rec.size_bytes >= 7_000_000_000 ||
      rec.id.includes("gemma-3-12b") ||
      rec.id.includes("tiger-gemma") ||
      rec.id.includes("gemma-3-27b") ||
      rec.id.includes("gemma-4-26") ||
      rec.id.includes("gemma-4-31")
    );
  }

  function isMobile4070Model(rec: { id: string; size_bytes: number }) {
    return rec.size_bytes <= 6_200_000_000 && !isRtx3090Model(rec);
  }

  function modelDownloadPercent(modelId: string): number {
    const d = modelsStore.download;
    if (!d || d.modelId !== modelId || d.total === 0) return 0;
    return Math.round((d.downloaded / d.total) * 100);
  }

  onMount(() => {
    settingsStore.load().catch((e) => console.warn("settings load selhal:", e));
    modelsStore.load().catch((e) => console.warn("models load selhal:", e));
    comfyInstallStore.load().catch((e) => console.warn("comfy status load selhal:", e));
    openvinoInstallStore.load().catch((e) => console.warn("openvino status load selhal:", e));
    getVersion()
      .then((v) => (appVersion = v))
      .catch((e) => console.warn("app version load selhal:", e));
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

  async function refreshRuntime() {
    await Promise.allSettled([
      settingsStore.load(),
      modelsStore.load(),
      comfyInstallStore.load(),
      openvinoInstallStore.load(),
    ]);
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

<div class="settings-overlay" class:window-mode={windowMode}>
  <div class="settings-card" class:window-mode={windowMode}>
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
        <button class:active={section === "runtime"} onclick={() => (section = "runtime")}>
          {i18n.m.settings.sections.runtime}
        </button>
        <button class:active={section === "downloads"} onclick={() => (section = "downloads")}>
          {i18n.m.settings.sections.downloads}
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
        <button class:active={section === "updates"} onclick={() => (section = "updates")}>
          {i18n.m.settings.sections.updates}
        </button>
        <button class:active={section === "backup"} onclick={() => (section = "backup")}>
          {i18n.m.settings.sections.backup}
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
                  <span class="key-status" class:set={state.hasKey} class:unset={!state.hasKey}>
                    <span class="status-dot"></span>
                    {state.hasKey ? (state.masked ?? i18n.m.settings.apiKeys.stored) : i18n.m.settings.apiKeys.notStored}
                  </span>
                </div>
                <div class="key-actions">
                  <input
                    type="password"
                    placeholder={state.hasKey ? i18n.m.settings.apiKeys.replacePlaceholder : i18n.m.wizard.steps.apiKeys.placeholder}
                    bind:value={keyInputs[svc.id]}
                    onkeydown={(e) => e.key === "Enter" && saveKey(svc.id)}
                  />
                  <button class="btn-sm primary" onclick={() => saveKey(svc.id)} disabled={!keyInputs[svc.id].trim()}>
                    {state.hasKey ? i18n.m.settings.apiKeys.update : i18n.m.common.save}
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
              class:selected={settingsStore.llmBackend === "openvino_npu"}
              onclick={() => settingsStore.setBackend("openvino_npu")}
            >
              {i18n.m.settings.llm.openvinoNpu}
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
                <button
                  class="btn-sm"
                  title={i18n.m.settings.llm.unloadVramHint}
                  disabled={unloadingVram}
                  onclick={unloadVram}
                >
                  {i18n.m.settings.llm.unloadVram}
                </button>
              </div>
              {#if vramUnloaded}
                <span class="conn-status connected">{i18n.m.settings.llm.unloadedVram}</span>
              {/if}
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
                          const path = modelsStore.models.find((m) => m.id === rec.id)?.path ?? "";
                          settingsStore.activateModel(path);
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

            <h4 class="sub-heading">{i18n.m.settings.llm.searchTitle}</h4>
            <p class="hint">{i18n.m.settings.llm.searchHint}</p>
            <div class="comfyui-row">
              <input
                type="text"
                placeholder={i18n.m.settings.llm.searchPlaceholder}
                value={modelSearchStore.query}
                oninput={(e) => modelSearchStore.setQuery((e.target as HTMLInputElement).value)}
                onkeydown={(e) => e.key === "Enter" && modelSearchStore.search()}
              />
              <button
                class="btn-sm primary"
                disabled={modelSearchStore.searching || !modelSearchStore.query.trim()}
                onclick={() => modelSearchStore.search()}
              >
                {modelSearchStore.searching
                  ? i18n.m.settings.llm.searching
                  : i18n.m.settings.llm.searchButton}
              </button>
            </div>

            {#if modelSearchStore.error}
              <span class="conn-status disconnected">{modelSearchStore.error}</span>
            {/if}

            {#if modelSearchStore.searched && modelSearchStore.results.length === 0 && !modelSearchStore.searching}
              <p class="hint">{i18n.m.settings.llm.searchEmpty}</p>
            {/if}

            {#if modelSearchStore.results.length > 0}
              <div class="recommended-list">
                {#each modelSearchStore.results as repo (repo.repo_id)}
                  {@const isOpen = modelSearchStore.expandedRepo === repo.repo_id}
                  <div class="recommended-item search-repo" class:active={isOpen}>
                    <div class="recommended-meta">
                      <div class="recommended-name">
                        {repo.name}
                        {#if repo.gated}
                          <span class="gated-badge" title={i18n.m.settings.llm.gatedHint}>🔒</span>
                        {/if}
                      </div>
                      <div class="recommended-desc">
                        {repo.author} · ⬇ {repo.downloads.toLocaleString()} · ❤ {repo.likes.toLocaleString()}
                      </div>
                      {#if isOpen}
                        {#if modelSearchStore.loadingFiles && modelSearchStore.filesFor(repo.repo_id).length === 0}
                          <div class="recommended-size">{i18n.m.common.loading}</div>
                        {:else if modelSearchStore.filesFor(repo.repo_id).length === 0}
                          <div class="recommended-size">{i18n.m.settings.llm.noGgufFiles}</div>
                        {:else}
                          <div class="quant-list">
                            {#each modelSearchStore.filesFor(repo.repo_id) as file (file.file_name)}
                              {@const fileId = modelIdForFile(file.file_name)}
                              {@const downloaded = modelsStore.isDownloaded(fileId)}
                              {@const vram = (modelsStore.gpu?.vram_mb ?? 0) * 1024 * 1024}
                              <div class="quant-row">
                                <span class="quant-badge">{file.quant ?? "GGUF"}</span>
                                <span class="quant-size">
                                  {formatBytes(file.size_bytes)}
                                  {#if vram > 0 && file.size_bytes > 0}
                                    {#if file.size_bytes < vram * 0.8}
                                      <span class="fit ok" title={i18n.m.settings.llm.vramFits}>●</span>
                                    {:else if file.size_bytes < vram}
                                      <span class="fit tight" title={i18n.m.settings.llm.vramTight}>●</span>
                                    {:else}
                                      <span class="fit over" title={i18n.m.settings.llm.vramOver}>●</span>
                                    {/if}
                                  {/if}
                                </span>
                                {#if downloaded}
                                  <span class="dl-inline">✓ {i18n.m.wizard.steps.models.ready}</span>
                                {:else if modelsStore.download?.modelId === fileId}
                                  <span class="dl-inline">
                                    {Math.round(((modelsStore.download.downloaded) / (modelsStore.download.total || 1)) * 100)}%
                                  </span>
                                {:else}
                                  <button
                                    class="btn-sm primary"
                                    disabled={!!modelsStore.download}
                                    onclick={() =>
                                      modelsStore.downloadModel(fileId, file.download_url, file.sha256)}
                                  >
                                    {i18n.m.settings.llm.download}
                                  </button>
                                {/if}
                              </div>
                            {/each}
                          </div>
                        {/if}
                      {/if}
                    </div>
                    <button class="btn-sm" onclick={() => modelSearchStore.toggleRepo(repo.repo_id)}>
                      {isOpen ? i18n.m.settings.llm.hideQuants : i18n.m.settings.llm.showQuants}
                    </button>
                  </div>
                {/each}
              </div>
            {/if}

            {#if modelsStore.download}
              <div class="dl-progress" style="margin-top:0.75rem">
                <div class="dl-head">
                  <span>{modelsStore.download.modelId}</span>
                  <span>
                    {#if modelsStore.download.phase === "verifying"}
                      {i18n.m.common.loading}
                    {:else}
                      {formatBytes(modelsStore.download.downloaded)} / {formatBytes(modelsStore.download.total)}{#if modelsStore.download.speedBytesPerSec > 0} · {formatSpeed(modelsStore.download.speedBytesPerSec)}{#if formatEta(modelsStore.download.total - modelsStore.download.downloaded, modelsStore.download.speedBytesPerSec)} · ⏱ {formatEta(modelsStore.download.total - modelsStore.download.downloaded, modelsStore.download.speedBytesPerSec)}{/if}{/if}
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

          {#if settingsStore.llmBackend === "openvino_npu"}
            <p class="hint" style="margin-top:1rem">{i18n.m.settings.llm.openvinoNpuHint}</p>
            <div class="npu-info">
              <div>
                <strong>{i18n.m.settings.llm.npuDevice}</strong>
                {#if settingsStore.npuInfo?.available}
                  <span>{settingsStore.npuInfo.name ?? i18n.m.settings.llm.npuDetected}</span>
                  {#if settingsStore.npuInfo.manufacturer}
                    <small>{settingsStore.npuInfo.manufacturer}</small>
                  {/if}
                {:else}
                  <span>{i18n.m.settings.llm.npuNotDetected}</span>
                {/if}
              </div>
              <button class="btn-sm" onclick={() => settingsStore.detectNpu()}>
                {i18n.m.settings.llm.detectNpu}
              </button>
            </div>
            <h4 class="sub-heading">{i18n.m.settings.llm.openvinoRuntimeTitle}</h4>
            <div class="runtime-card">
              <div>
                <strong>
                  {openvinoInstallStore.status?.installed
                    ? i18n.m.settings.llm.openvinoRuntimeInstalled
                    : i18n.m.settings.llm.openvinoRuntimeMissing}
                </strong>
                {#if openvinoInstallStore.status}
                  <small>{openvinoInstallStore.status.installDir}</small>
                {/if}
              </div>
              {#if openvinoInstallStore.status?.installed}
                <button
                  class="btn-sm danger"
                  disabled={openvinoInstallStore.uninstalling || openvinoInstallStore.installing}
                  onclick={() => openvinoInstallStore.uninstall()}
                >
                  {openvinoInstallStore.uninstalling ? i18n.m.common.loading : i18n.m.settings.llm.openvinoRuntimeUninstall}
                </button>
              {:else}
                <button
                  class="btn-sm primary"
                  disabled={openvinoInstallStore.installing}
                  onclick={() => openvinoInstallStore.install()}
                >
                  {openvinoInstallStore.installing ? i18n.m.common.loading : i18n.m.settings.llm.openvinoRuntimeInstall}
                </button>
              {/if}
            </div>
            {#if openvinoInstallStore.installing}
              <div class="install-progress">
                <div class="install-step">
                  <span class="spinner"></span>
                  <span>{openvinoInstallStore.currentStep || i18n.m.common.loading}</span>
                </div>
                <pre class="install-log">{openvinoInstallStore.log.join("\n")}</pre>
              </div>
            {/if}
            {#if openvinoInstallStore.error}
              <span class="conn-status disconnected">{openvinoInstallStore.error}</span>
            {/if}
            {#if openvinoInstallStore.npuMissing}
              <p class="npu-warning">
                {i18n.m.settings.llm.openvinoNoNpuWarning}
                {#if openvinoInstallStore.availableDevices.length > 0}
                  <span>{openvinoInstallStore.availableDevices.join(", ")}</span>
                {/if}
              </p>
            {/if}
            {#if openvinoInstallStore.status?.installed}
              <label class="field-label" for="openvino-device" style="margin-top:1rem">
                {i18n.m.settings.llm.openvinoDevice}
              </label>
              <div class="comfyui-row">
                <select
                  id="openvino-device"
                  value={openvinoInstallStore.device}
                  disabled={openvinoInstallStore.status.serverRunning}
                  onchange={(e) => openvinoInstallStore.setDevice((e.target as HTMLSelectElement).value)}
                >
                  {#each openvinoInstallStore.deviceOptions as dev (dev)}
                    <option value={dev}>{dev}</option>
                  {/each}
                </select>
              </div>
              <p class="hint" style="margin-top:0.35rem">{i18n.m.settings.llm.openvinoDeviceHint}</p>
              <label class="field-label" for="openvino-model-profile" style="margin-top:1rem">
                {i18n.m.settings.llm.openvinoModelProfile}
              </label>
              <div class="comfyui-row">
                <select
                  id="openvino-model-profile"
                  value={openvinoInstallStore.selectedProfileId}
                  onchange={(e) => openvinoInstallStore.setSelectedProfile((e.target as HTMLSelectElement).value)}
                >
                  {#each openvinoInstallStore.profilesForDevice as profile (profile.id)}
                    <option value={profile.id}>
                      {profile.name} - {profile.qualityTier}
                    </option>
                  {/each}
                </select>
                {#if openvinoInstallStore.selectedProfile?.sourceUrl}
                  <button class="btn-sm" onclick={openSelectedOpenvinoSource}>
                    {i18n.m.settings.llm.openvinoOpenSource}
                  </button>
                {/if}
              </div>
              {#if openvinoInstallStore.selectedProfile}
                <div class="npu-profile-card">
                  <strong>{openvinoInstallStore.selectedProfile.name}</strong>
                  <span>{openvinoInstallStore.selectedProfile.description}</span>
                  <small>
                    {openvinoInstallStore.selectedProfile.autoDownloadable
                      ? i18n.m.settings.llm.openvinoProfileAuto
                      : i18n.m.settings.llm.openvinoProfileManual}
                    - {openvinoInstallStore.selectedProfile.sizeHint}
                    - {i18n.m.settings.llm.openvinoProfileDevices}: {openvinoInstallStore.selectedProfile.supportedDevices.join(", ")}
                  </small>
                </div>
              {/if}
              <label class="field-label" for="openvino-model-dir" style="margin-top:1rem">
                {i18n.m.settings.llm.openvinoModelDir}
              </label>
              <div class="comfyui-row">
                <input
                  id="openvino-model-dir"
                  type="text"
                  value={openvinoInstallStore.modelDir}
                  placeholder={openvinoInstallStore.status.defaultModelDir}
                  oninput={(e) => openvinoInstallStore.setModelDir((e.target as HTMLInputElement).value)}
                />
                <button
                  class="btn-sm"
                  onclick={pickOpenvinoModelDir}
                >
                  {i18n.m.settings.llm.browse}
                </button>
                <button
                  class="btn-sm"
                  disabled={openvinoInstallStore.downloadingModel || !openvinoInstallStore.selectedProfile?.autoDownloadable}
                  onclick={() => openvinoInstallStore.downloadRecommendedModel()}
                >
                  {openvinoInstallStore.downloadingModel ? i18n.m.common.loading : i18n.m.settings.llm.openvinoDownloadSelected}
                </button>
              </div>
              <p class="hint" style="margin-top:0.35rem">{i18n.m.settings.llm.openvinoModelHint}</p>
              <div class="comfy-status-row">
                {#if openvinoInstallStore.status.serverRunning}
                  <span class="conn-status connected">● {i18n.m.settings.llm.openvinoServerRunning}</span>
                  <button
                    class="btn-sm danger"
                    disabled={openvinoInstallStore.stoppingServer}
                    onclick={() => openvinoInstallStore.stopServer()}
                  >
                    {openvinoInstallStore.stoppingServer ? i18n.m.common.loading : i18n.m.settings.llm.openvinoStopServer}
                  </button>
                {:else}
                  <span class="conn-status testing">{i18n.m.settings.llm.openvinoServerStopped}</span>
                  <button
                    class="btn-sm primary"
                    disabled={openvinoInstallStore.startingServer || !openvinoInstallStore.modelDir.trim()}
                    onclick={() => openvinoInstallStore.startServer()}
                  >
                    {openvinoInstallStore.startingServer ? i18n.m.common.loading : i18n.m.settings.llm.openvinoStartServer}
                  </button>
                {/if}
              </div>
              {#if openvinoInstallStore.status.serverLogPath}
                <p class="hint" style="margin-top:0.5rem">
                  {i18n.m.settings.llm.openvinoLog}: {openvinoInstallStore.status.serverLogPath}
                </p>
              {/if}
            {/if}
            <label class="field-label" for="openvino-npu-url" style="margin-top:1rem">
              {i18n.m.settings.llm.openvinoNpuUrl}
            </label>
            <div class="comfyui-row">
              <input
                id="openvino-npu-url"
                type="text"
                value={settingsStore.openvinoNpuUrl}
                oninput={(e) =>
                  settingsStore.setOpenvinoNpuUrl((e.target as HTMLInputElement).value)}
                onblur={() => settingsStore.saveOpenvinoNpuUrl()}
              />
              <button class="btn-sm primary" onclick={() => settingsStore.testOpenvinoNpu()}>
                {i18n.m.settings.llm.test}
              </button>
            </div>
            {#if settingsStore.openvinoNpuStatus === "connected"}
              <span class="conn-status connected">â—Ź {i18n.m.settings.llm.connected}</span>
            {:else if settingsStore.openvinoNpuStatus === "disconnected"}
              <span class="conn-status disconnected">â—Ź {i18n.m.settings.llm.disconnected}</span>
            {:else if settingsStore.openvinoNpuStatus === "testing"}
              <span class="conn-status testing">{i18n.m.common.loading}</span>
            {/if}
          {/if}
        {:else if section === "runtime"}
          <div class="section-title-row">
            <h3>{i18n.m.settings.runtime.title}</h3>
            <div class="section-actions">
              <button class="btn-sm" onclick={refreshRuntime}>{i18n.m.settings.runtime.refresh}</button>
              <button class="btn-sm primary" disabled={restartingRuntime} onclick={restartRuntime}>
                {restartingRuntime ? i18n.m.common.loading : i18n.m.settings.runtime.restartRuntime}
              </button>
            </div>
          </div>
          {#if runtimeRestartNotice}
            <p class="hint">{runtimeRestartNotice}</p>
          {/if}
          <div class="runtime-grid">
            <section class="runtime-card">
              <div>
                <strong>{i18n.m.settings.runtime.llm}</strong>
                <small>{settingsStore.llmBackend}</small>
              </div>
              <span class="conn-status testing">{modelsStore.models.length} {i18n.m.settings.runtime.localModels}</span>
              {#if modelsStore.download}
                <div class="dl-progress runtime-download">
                  <div class="dl-head">
                    <span>{modelsStore.download.modelId}</span>
                    <span>{downloadPercent()}%</span>
                  </div>
                  <div class="dl-bar">
                    <div class="dl-fill" style="width: {downloadPercent()}%"></div>
                  </div>
                  <small>
                    {formatBytes(modelsStore.download.downloaded)} / {formatBytes(modelsStore.download.total)}
                    {#if modelsStore.download.speedBytesPerSec > 0}
                      · {formatSpeed(modelsStore.download.speedBytesPerSec)}
                    {/if}
                  </small>
                </div>
              {/if}
              <div class="runtime-actions">
                <button class="btn-sm" onclick={() => (section = "models")}>{i18n.m.settings.sections.models}</button>
                <button class="btn-sm danger" disabled={unloadingVram} onclick={unloadVram}>
                  {unloadingVram ? i18n.m.common.loading : i18n.m.settings.runtime.unloadModel}
                </button>
              </div>
            </section>

            <section class="runtime-card">
              <div>
                <strong>ComfyUI</strong>
                <small>{settingsStore.comfyuiUrl}</small>
              </div>
              {#if comfyInstallStore.status === "Running"}
                <span class="conn-status connected">● {i18n.m.settings.runtime.running}</span>
              {:else if comfyInstallStore.status === "Installed"}
                <span class="conn-status testing">● {i18n.m.settings.runtime.installedStopped}</span>
              {:else if comfyInstallStore.status === "Broken"}
                <span class="conn-status disconnected">● {i18n.m.settings.comfyui.brokenLabel}</span>
              {:else}
                <span class="conn-status disconnected">● {i18n.m.settings.runtime.notInstalled}</span>
              {/if}
              <div class="runtime-actions">
                {#if comfyInstallStore.status === "Running"}
                  <button class="btn-sm danger" onclick={() => comfyInstallStore.stopServer()}>
                    {i18n.m.settings.comfyui.stopServer}
                  </button>
                {:else if comfyInstallStore.status === "Installed"}
                  <button class="btn-sm primary" disabled={comfyInstallStore.starting} onclick={() => comfyInstallStore.startServer()}>
                    {comfyInstallStore.starting ? i18n.m.common.loading : i18n.m.settings.comfyui.startServer}
                  </button>
                {:else if comfyInstallStore.status === "Broken"}
                  <button class="btn-sm primary" disabled={comfyInstallStore.installing} onclick={() => comfyInstallStore.install()}>
                    {comfyInstallStore.installing ? i18n.m.common.loading : i18n.m.settings.comfyui.repair}
                  </button>
                {:else}
                  <button class="btn-sm primary" disabled={comfyInstallStore.installing} onclick={() => comfyInstallStore.install()}>
                    {comfyInstallStore.installing ? i18n.m.common.loading : i18n.m.settings.comfyui.installButton}
                  </button>
                {/if}
                <button class="btn-sm" onclick={() => (section = "comfyui")}>{i18n.m.settings.runtime.details}</button>
              </div>
              {#if comfyInstallStore.error}
                <span class="conn-status disconnected">{comfyInstallStore.error}</span>
              {/if}
            </section>

            <section class="runtime-card">
              <div>
                <strong>OpenVINO / NPU</strong>
                <small>{openvinoInstallStore.status?.installDir ?? "..."}</small>
              </div>
              {#if openvinoInstallStore.status?.serverRunning}
                <span class="conn-status connected">● {i18n.m.settings.runtime.running}</span>
              {:else if openvinoInstallStore.status?.installed}
                <span class="conn-status testing">● {i18n.m.settings.runtime.installedStopped}</span>
              {:else}
                <span class="conn-status disconnected">● {i18n.m.settings.runtime.notInstalled}</span>
              {/if}
              <div class="runtime-actions">
                {#if openvinoInstallStore.status?.serverRunning}
                  <button class="btn-sm danger" disabled={openvinoInstallStore.stoppingServer} onclick={() => openvinoInstallStore.stopServer()}>
                    {openvinoInstallStore.stoppingServer ? i18n.m.common.loading : i18n.m.settings.llm.openvinoStopServer}
                  </button>
                {:else if openvinoInstallStore.status?.installed}
                  <button class="btn-sm primary" disabled={openvinoInstallStore.startingServer || !openvinoInstallStore.modelDir.trim()} onclick={() => openvinoInstallStore.startServer()}>
                    {openvinoInstallStore.startingServer ? i18n.m.common.loading : i18n.m.settings.llm.openvinoStartServer}
                  </button>
                {:else}
                  <button class="btn-sm primary" disabled={openvinoInstallStore.installing} onclick={() => openvinoInstallStore.install()}>
                    {openvinoInstallStore.installing ? i18n.m.common.loading : i18n.m.settings.llm.openvinoRuntimeInstall}
                  </button>
                {/if}
                <button class="btn-sm" onclick={() => (section = "llm")}>{i18n.m.settings.runtime.details}</button>
              </div>
              {#if openvinoInstallStore.error}
                <span class="conn-status disconnected">{openvinoInstallStore.error}</span>
              {/if}
            </section>
          </div>
        {:else if section === "downloads"}
          <div class="section-title-row">
            <h3>{i18n.m.settings.downloads.title}</h3>
            <button class="btn-sm" onclick={refreshRuntime}>{i18n.m.settings.runtime.refresh}</button>
          </div>
          <section class="download-tuning">
            <label for="download-segments">
              <span>{i18n.m.settings.downloads.segments}</span>
              <strong>{modelsStore.downloadSegments}</strong>
            </label>
            <input
              id="download-segments"
              type="range"
              min="1"
              max="32"
              step="1"
              value={modelsStore.downloadSegments}
              oninput={(e) => modelsStore.setDownloadSegments(Number((e.target as HTMLInputElement).value))}
            />
            <p class="hint">{i18n.m.settings.downloads.segmentsHint}</p>
          </section>
          <div class="download-manager">
            {#if modelsStore.download}
              <section class="download-row">
                <div>
                  <strong>{modelsStore.download.modelId}</strong>
                  <small>{modelsStore.download.phase === "verifying" ? i18n.m.settings.downloads.verifying : i18n.m.settings.downloads.llmModel}</small>
                </div>
                <div class="download-row-main">
                  <div class="dl-head">
                    <span>{downloadPercent()}%</span>
                    <span>
                      {formatBytes(modelsStore.download.downloaded)} / {formatBytes(modelsStore.download.total)}
                      {#if modelsStore.download.speedBytesPerSec > 0}
                        · {formatSpeed(modelsStore.download.speedBytesPerSec)}
                      {/if}
                      {#if formatEta(modelsStore.download.total - modelsStore.download.downloaded, modelsStore.download.speedBytesPerSec)}
                        · {formatEta(modelsStore.download.total - modelsStore.download.downloaded, modelsStore.download.speedBytesPerSec)}
                      {/if}
                    </span>
                  </div>
                  <div class="dl-bar">
                    <div class="dl-fill" style="width: {downloadPercent()}%"></div>
                  </div>
                </div>
              </section>
            {/if}

            {#if comfyInstallStore.installing}
              <section class="download-row">
                <div>
                  <strong>ComfyUI</strong>
                  <small>{comfyInstallStore.currentStep || i18n.m.common.loading}</small>
                </div>
                <pre class="install-log compact-log">{comfyInstallStore.log.slice(-12).join("\n")}</pre>
              </section>
            {/if}

            {#if openvinoInstallStore.installing || openvinoInstallStore.downloadingModel}
              <section class="download-row">
                <div>
                  <strong>OpenVINO / NPU</strong>
                  <small>{openvinoInstallStore.downloadingModel ? i18n.m.settings.downloads.openvinoModel : (openvinoInstallStore.currentStep || i18n.m.common.loading)}</small>
                </div>
                {#if openvinoInstallStore.installing}
                  <pre class="install-log compact-log">{openvinoInstallStore.log.slice(-12).join("\n")}</pre>
                {:else}
                  <span class="conn-status testing">{i18n.m.common.loading}</span>
                {/if}
              </section>
            {/if}

            {#if !modelsStore.download && !comfyInstallStore.installing && !openvinoInstallStore.installing && !openvinoInstallStore.downloadingModel}
              <p class="hint">{i18n.m.settings.downloads.empty}</p>
            {/if}
          </div>
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
          {:else if comfyInstallStore.status === "Broken"}
            <div class="comfy-status-row">
              <span class="conn-status disconnected">● {i18n.m.settings.comfyui.brokenLabel}</span>
              <button class="btn-sm primary" onclick={() => comfyInstallStore.install()}>
                {i18n.m.settings.comfyui.repair}
              </button>
              <button
                class="btn-sm danger"
                disabled={comfyInstallStore.uninstalling}
                onclick={uninstallComfyui}
              >
                {comfyInstallStore.uninstalling ? i18n.m.settings.comfyui.uninstalling : i18n.m.settings.comfyui.uninstall}
              </button>
            </div>
            <p class="hint">{i18n.m.settings.comfyui.brokenHint}</p>
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
              <button
                class="btn-sm danger"
                disabled={comfyInstallStore.uninstalling}
                onclick={uninstallComfyui}
              >
                {comfyInstallStore.uninstalling ? i18n.m.settings.comfyui.uninstalling : i18n.m.settings.comfyui.uninstall}
              </button>
            </div>
          {:else if comfyInstallStore.status === "Running"}
            <div class="comfy-status-row">
              <span class="conn-status connected">● {i18n.m.settings.comfyui.runningLabel}</span>
              <button class="btn-sm danger" onclick={() => comfyInstallStore.stopServer()}>
                {i18n.m.settings.comfyui.stopServer}
              </button>
              <button
                class="btn-sm danger"
                disabled={comfyInstallStore.uninstalling}
                onclick={uninstallComfyui}
              >
                {comfyInstallStore.uninstalling ? i18n.m.settings.comfyui.uninstalling : i18n.m.settings.comfyui.uninstall}
              </button>
            </div>
          {/if}

          {#if comfyInstallStore.error}
            <span class="conn-status disconnected">{comfyInstallStore.error}</span>
          {/if}

          <h4 class="sub-heading">{i18n.m.settings.comfyui.diagnosticsTitle}</h4>
          <button
            class="btn-sm"
            disabled={comfyInstallStore.diagnosing}
            onclick={() => comfyInstallStore.diagnose()}
          >
            {comfyInstallStore.diagnosing ? i18n.m.common.loading : i18n.m.settings.comfyui.diagnosticsRun}
          </button>

          {#if comfyInstallStore.diagnostics}
            <div class="diagnostics-card">
              <div class="diag-path">
                <strong>{i18n.m.settings.comfyui.installDir}</strong>
                <span>{comfyInstallStore.diagnostics.install_dir}</span>
              </div>
              <div class:ok={comfyInstallStore.diagnostics.main_py_exists}>
                <span>main.py</span>
                <strong>{diagStatus(comfyInstallStore.diagnostics.main_py_exists)}</strong>
              </div>
              <div class:ok={comfyInstallStore.diagnostics.requirements_exists}>
                <span>requirements.txt</span>
                <strong>{diagStatus(comfyInstallStore.diagnostics.requirements_exists)}</strong>
              </div>
              <div class:ok={comfyInstallStore.diagnostics.venv_python_exists}>
                <span>venv Python</span>
                <strong>{diagStatus(comfyInstallStore.diagnostics.venv_python_exists)}</strong>
              </div>
              <div class:ok={comfyInstallStore.diagnostics.pulid_node_exists}>
                <span>PuLID custom node</span>
                <strong>{diagStatus(comfyInstallStore.diagnostics.pulid_node_exists)}</strong>
              </div>
              <div class:ok={comfyInstallStore.diagnostics.impact_pack_exists}>
                <span>Impact Pack</span>
                <strong>{diagStatus(comfyInstallStore.diagnostics.impact_pack_exists)}</strong>
              </div>
              <div class="diag-path">
                <strong>{i18n.m.settings.comfyui.serverLog}</strong>
                <span>{comfyInstallStore.diagnostics.server_log_path}</span>
              </div>
            </div>
            {#if comfyInstallStore.diagnostics.server_log_tail}
              <pre class="install-log">{comfyInstallStore.diagnostics.server_log_tail}</pre>
            {/if}
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

            <h4 class="sub-heading">{i18n.m.settings.civitai.title}</h4>
            <p class="hint">{i18n.m.settings.civitai.hint}</p>
            <div class="comfyui-row">
              <select
                class="civitai-kind"
                value={civitaiBrowserStore.kind}
                onchange={(e) =>
                  civitaiBrowserStore.setKind(
                    (e.target as HTMLSelectElement).value as "checkpoint" | "lora"
                  )}
              >
                <option value="checkpoint">{i18n.m.settings.civitai.kindCheckpoint}</option>
                <option value="lora">{i18n.m.settings.civitai.kindLora}</option>
              </select>
              <input
                type="text"
                placeholder={i18n.m.settings.civitai.searchPlaceholder}
                value={civitaiBrowserStore.query}
                oninput={(e) => civitaiBrowserStore.setQuery((e.target as HTMLInputElement).value)}
                onkeydown={(e) => e.key === "Enter" && civitaiBrowserStore.search()}
              />
              <button
                class="btn-sm primary"
                disabled={civitaiBrowserStore.searching || !civitaiBrowserStore.query.trim()}
                onclick={() => civitaiBrowserStore.search()}
              >
                {civitaiBrowserStore.searching
                  ? i18n.m.settings.civitai.searching
                  : i18n.m.settings.civitai.searchButton}
              </button>
            </div>

            {#if civitaiBrowserStore.error}
              <span class="conn-status disconnected">{civitaiBrowserStore.error}</span>
            {/if}
            {#if civitaiBrowserStore.searched && civitaiBrowserStore.results.length === 0 && !civitaiBrowserStore.searching}
              <p class="hint">{i18n.m.settings.civitai.empty}</p>
            {/if}

            {#if civitaiBrowserStore.results.length > 0}
              <div class="civitai-grid">
                {#each civitaiBrowserStore.results as item (item.file_name)}
                  {@const isDownloading = civitaiBrowserStore.downloadingFile === item.file_name}
                  <div class="civitai-card">
                    {#if item.preview_image_url}
                      <img
                        class="civitai-preview"
                        src={item.preview_image_url}
                        alt={item.name}
                        loading="lazy"
                      />
                    {:else}
                      <div class="civitai-preview civitai-noimg">🖼</div>
                    {/if}
                    <div class="civitai-meta">
                      <div class="civitai-name" title={item.name}>
                        {item.name}
                        {#if item.nsfw}<span class="nsfw-badge">18+</span>{/if}
                      </div>
                      <div class="civitai-sub">
                        {item.creator} · ⬇ {item.downloads.toLocaleString()}
                      </div>
                      <div class="civitai-sub">
                        {item.base_model} · {formatBytes(item.size_bytes)}
                      </div>
                      {#if item.trigger_words.length > 0}
                        <div class="civitai-triggers" title={item.trigger_words.join(", ")}>
                          🔑 {item.trigger_words.join(", ")}
                        </div>
                      {/if}
                      {#if civitaiBrowserStore.isDownloaded(item.file_name)}
                        <span class="dl-inline">✓ {i18n.m.wizard.steps.models.ready}</span>
                      {:else if isDownloading}
                        <span class="dl-inline">{civitaiBrowserStore.progressLine || i18n.m.common.loading}</span>
                      {:else}
                        <button
                          class="btn-sm primary"
                          disabled={!!civitaiBrowserStore.downloadingFile}
                          onclick={() => civitaiBrowserStore.download(item).then(() => comfyInstallStore.load())}
                        >
                          {i18n.m.settings.llm.download}
                        </button>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            {/if}
          {/if}
        {:else if section === "models"}
          <h3>{i18n.m.settings.models.title}</h3>

          <!-- Chyba patri nahoru k akcim. Driv byla az pod obema seznamy modelu,
               takze pri kliknuti nahore zustala mimo obrazovku a stahovani
               vypadalo, ze "nic nedela". -->
          {#if modelsStore.error}
            <p class="conn-status disconnected" style="margin-bottom:0.75rem">{modelsStore.error}</p>
          {/if}

          <label class="field-label" for="models-dir">{i18n.m.settings.models.dirLabel}</label>
          <div class="comfyui-row">
            <input id="models-dir" type="text" readonly value={modelsStore.modelsDir} />
            <button
              class="btn-sm primary"
              disabled={modelsStore.movingModelsDir}
              onclick={pickModelsDir}
            >
              {i18n.m.settings.models.dirBrowse}
            </button>
          </div>
          {#if modelsStore.movingModelsDir}
            <p class="hint">{i18n.m.settings.models.dirMoving}</p>
          {/if}

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

          <h4 class="sub-heading">{i18n.m.settings.models.recommendedMobile}</h4>
          <div class="recommended-list">
            {#each modelsStore.recommended.filter(isMobile4070Model) as rec (rec.id)}
              {@const downloaded = modelsStore.isDownloaded(rec.id)}
              {@const isActive = settingsStore.modelPath.endsWith(`${rec.id}.gguf`)}
              <div class="recommended-item" class:active={isActive}>
                <div class="recommended-meta">
                  <div class="recommended-name">
                    {rec.name}
                    <span class="model-chip">{formatBytes(rec.size_bytes)}</span>
                    {#if isActive}<span class="active-badge">{i18n.m.settings.llm.inUse}</span>{/if}
                  </div>
                  <div class="recommended-desc">{rec.description}</div>
                </div>
                {#if downloaded}
                  <div class="recommended-actions">
                    <button
                      class="btn-sm"
                      class:primary={!isActive}
                      disabled={isActive}
                      onclick={() => {
                        const path = modelsStore.models.find((m) => m.id === rec.id)?.path ?? "";
                        settingsStore.activateModel(path);
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
                  <span class="dl-inline">{modelDownloadPercent(rec.id)}%</span>
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

          <h4 class="sub-heading">{i18n.m.settings.models.recommendedRtx3090}</h4>
          <div class="recommended-list">
            {#each modelsStore.recommended.filter(isRtx3090Model) as rec (rec.id)}
              {@const downloaded = modelsStore.isDownloaded(rec.id)}
              {@const isActive = settingsStore.modelPath.endsWith(`${rec.id}.gguf`)}
              <div class="recommended-item" class:active={isActive}>
                <div class="recommended-meta">
                  <div class="recommended-name">
                    {rec.name}
                    <span class="model-chip">{formatBytes(rec.size_bytes)}</span>
                    {#if isActive}<span class="active-badge">{i18n.m.settings.llm.inUse}</span>{/if}
                  </div>
                  <div class="recommended-desc">{rec.description}</div>
                </div>
                {#if downloaded}
                  <div class="recommended-actions">
                    <button
                      class="btn-sm"
                      class:primary={!isActive}
                      disabled={isActive}
                      onclick={() => {
                        const path = modelsStore.models.find((m) => m.id === rec.id)?.path ?? "";
                        settingsStore.activateModel(path);
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
                  <span class="dl-inline">{modelDownloadPercent(rec.id)}%</span>
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

          <h4 class="sub-heading">{i18n.m.settings.models.downloadedTitle}</h4>
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
          <button
            class="open-log-window-btn"
            onclick={() => invoke("open_log_window").catch((e) => console.warn(e))}
          >
            🗗 {i18n.m.settings.logs.openWindow}
          </button>
          <LogViewer />
        {:else if section === "backup"}
          <h3>{i18n.m.settings.sections.backup}</h3>
          <p class="hint">{i18n.m.settings.backup.hint}</p>

          <div class="option-row">
            <button class="btn-sm primary" disabled={backupBusy} onclick={exportBackup}>
              {backupBusy ? i18n.m.common.loading : i18n.m.settings.backup.exportButton}
            </button>
            <button class="btn-sm" disabled={backupBusy || restorePending} onclick={importBackup}>
              {i18n.m.settings.backup.importButton}
            </button>
          </div>

          {#if backupNotice}
            <span class="conn-status connected">{backupNotice}</span>
          {/if}
          {#if backupError}
            <span class="conn-status disconnected">{backupError}</span>
          {/if}
          {#if restorePending}
            <p class="hint">{i18n.m.settings.backup.restartRequired}</p>
            <button class="btn-sm primary" onclick={() => relaunch()}>
              {i18n.m.settings.updates.restartNow}
            </button>
          {/if}
        {:else if section === "updates"}
          <h3>{i18n.m.settings.sections.updates}</h3>
          <div class="version-row">
            <span class="version-label">{i18n.m.settings.updates.currentVersion}</span>
            <span class="version-value">{appVersion || "…"}</span>
          </div>

          {#if updaterStore.phase === "downloading"}
            <div class="dl-progress" style="margin-top:0.75rem">
              <div class="dl-head">
                <span>{i18n.m.settings.updates.downloading}</span>
                <span>{updaterStore.percent}%</span>
              </div>
              <div class="dl-bar">
                <div class="dl-fill" style="width: {updaterStore.percent}%"></div>
              </div>
            </div>
          {:else if updaterStore.phase === "readyToRestart"}
            <p class="hint" style="margin-top:0.75rem">{i18n.m.settings.updates.readyToRestart}</p>
            <button class="btn-sm primary" onclick={() => updaterStore.restart()}>
              {i18n.m.settings.updates.restartNow}
            </button>
          {:else if updaterStore.phase === "available"}
            <div class="update-available">
              <span class="conn-status connected">
                ● {i18n.t("settings.updates.available", { version: updaterStore.version ?? "" })}
              </span>
              {#if updaterStore.notes}
                <pre class="update-notes">{updaterStore.notes}</pre>
              {/if}
              <button class="btn-sm primary" onclick={() => updaterStore.downloadAndInstall()}>
                {i18n.m.settings.updates.downloadInstall}
              </button>
            </div>
          {:else}
            <button
              class="btn-sm primary"
              style="margin-top:0.75rem"
              disabled={updaterStore.phase === "checking"}
              onclick={() => updaterStore.checkForUpdate()}
            >
              {updaterStore.phase === "checking"
                ? i18n.m.settings.updates.checking
                : i18n.m.settings.updates.checkNow}
            </button>
            {#if updaterStore.phase === "upToDate"}
              <span class="conn-status connected">● {i18n.m.settings.updates.upToDate}</span>
            {:else if updaterStore.phase === "error"}
              <span class="conn-status disconnected">{updaterStore.error}</span>
            {/if}
          {/if}
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

  .settings-overlay.window-mode {
    z-index: 1;
    background: var(--color-bg);
    align-items: stretch;
    justify-content: stretch;
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

  .settings-card.window-mode {
    width: 100vw;
    max-width: none;
    height: 100vh;
    max-height: none;
    border: none;
    border-radius: 0;
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

  .open-log-window-btn {
    align-self: flex-start;
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.35rem 0.8rem;
    font-size: 0.82rem;
    cursor: pointer;
    margin-bottom: 0.6rem;
  }
  .open-log-window-btn:hover {
    border-color: var(--color-accent);
    color: var(--color-accent);
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
    align-items: center;
    border: 1px solid var(--color-border);
    border-radius: 999px;
    display: inline-flex;
    font-size: 0.78rem;
    font-weight: 700;
    gap: 0.35rem;
    padding: 0.18rem 0.55rem;
    white-space: nowrap;
  }
  .key-status.set {
    background: color-mix(in srgb, var(--color-success) 12%, transparent);
    border-color: color-mix(in srgb, var(--color-success) 55%, var(--color-border));
    color: var(--color-success);
  }
  .key-status.unset {
    background: var(--color-surface-2);
    color: var(--color-text-muted);
  }
  .status-dot {
    background: currentColor;
    border-radius: 999px;
    display: inline-block;
    height: 0.42rem;
    width: 0.42rem;
  }

  .key-actions {
    display: flex;
    gap: 0.5rem;
  }

  .key-actions input,
  .comfyui-row input,
  .comfyui-row select {
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
  .comfyui-row input:focus,
  .comfyui-row select:focus {
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

  .npu-info {
    align-items: center;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    display: flex;
    gap: 0.75rem;
    justify-content: space-between;
    padding: 0.6rem 0.8rem;
  }

  .npu-profile-card {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin-top: 0.5rem;
    padding: 0.7rem 0.8rem;
  }

  .npu-profile-card span,
  .npu-profile-card small {
    color: var(--color-text-muted);
    font-size: 0.8rem;
    line-height: 1.45;
  }

  .runtime-card {
    align-items: center;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    display: flex;
    gap: 0.75rem;
    justify-content: space-between;
    padding: 0.6rem 0.8rem;
  }
  .section-title-row {
    align-items: center;
    display: flex;
    gap: 0.75rem;
    justify-content: space-between;
  }

  .section-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.45rem;
    justify-content: flex-end;
  }

  .runtime-grid {
    display: grid;
    gap: 0.75rem;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    margin-top: 0.75rem;
  }

  .runtime-grid .runtime-card {
    align-items: stretch;
    flex-direction: column;
    justify-content: flex-start;
  }

  .runtime-card > div:first-child {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  .runtime-card strong {
    font-size: 0.84rem;
  }
  .runtime-card small {
    color: var(--color-text-muted);
    font-size: 0.72rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .runtime-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.45rem;
  }
  .runtime-download {
    margin-top: 0;
  }

  .download-manager {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    margin-top: 0.75rem;
  }

  .download-tuning {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    margin-top: 0.75rem;
    padding: 0.75rem;
  }

  .download-tuning label {
    align-items: center;
    display: flex;
    font-size: 0.85rem;
    justify-content: space-between;
  }

  .download-tuning input {
    width: 100%;
  }

  .download-row {
    align-items: stretch;
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    display: grid;
    gap: 0.75rem;
    grid-template-columns: minmax(170px, 230px) minmax(0, 1fr);
    padding: 0.75rem;
  }

  .download-row > div:first-child {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }

  .download-row strong {
    font-size: 0.85rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .download-row small {
    color: var(--color-text-muted);
    font-size: 0.73rem;
  }

  .download-row-main {
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    min-width: 0;
  }

  .compact-log {
    margin: 0;
    max-height: 150px;
  }
  .npu-info div {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }
  .npu-info strong {
    font-size: 0.75rem;
    color: var(--color-text-muted);
    text-transform: uppercase;
  }
  .npu-info span {
    font-size: 0.84rem;
    font-weight: 600;
  }
  .npu-info small {
    color: var(--color-text-muted);
    font-size: 0.74rem;
    overflow: hidden;
    text-overflow: ellipsis;
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
  .model-chip {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 999px;
    color: var(--color-text-muted);
    font-size: 0.68rem;
    font-weight: 600;
    padding: 0.05rem 0.4rem;
    white-space: nowrap;
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

  /* Vyhledávání na HuggingFace: rozbalený repo + řádky kvantizací */
  .search-repo {
    align-items: flex-start;
  }
  .gated-badge {
    font-size: 0.72rem;
  }
  .quant-list {
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    margin-top: 0.5rem;
  }
  .quant-row {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    font-size: 0.8rem;
  }
  .quant-badge {
    font-family: var(--font-mono, monospace);
    font-size: 0.7rem;
    font-weight: 600;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 4px;
    padding: 0.1rem 0.4rem;
    min-width: 4.5rem;
    text-align: center;
  }
  .quant-size {
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
    min-width: 6.5rem;
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }
  .fit {
    font-size: 0.7rem;
  }
  .fit.ok {
    color: #3fb950;
  }
  .fit.tight {
    color: #d29922;
  }
  .fit.over {
    color: var(--color-error);
  }

  /* CivitAI prohlížeč: karty s náhledy */
  .civitai-kind {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.45rem 0.5rem;
    font-size: 0.85rem;
  }
  .civitai-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(210px, 1fr));
    gap: 0.75rem;
    margin-top: 0.75rem;
  }
  .civitai-card {
    border: 1px solid var(--color-border);
    border-radius: 10px;
    background: var(--color-surface-2);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .civitai-preview {
    width: 100%;
    height: 200px;
    object-fit: cover;
    display: block;
    background: var(--color-surface);
  }
  .civitai-noimg {
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 2rem;
    color: var(--color-text-muted);
  }
  .civitai-meta {
    padding: 0.5rem 0.65rem 0.65rem;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }
  .civitai-name {
    font-size: 0.82rem;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .nsfw-badge {
    font-size: 0.62rem;
    font-weight: 700;
    color: var(--color-error);
    border: 1px solid var(--color-error);
    border-radius: 4px;
    padding: 0 0.25rem;
    margin-left: 0.3rem;
    vertical-align: middle;
  }
  .civitai-sub {
    font-size: 0.72rem;
    color: var(--color-text-muted);
  }
  .civitai-triggers {
    font-size: 0.7rem;
    color: var(--color-text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
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

  .diagnostics-card {
    background: var(--color-surface-2);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    display: grid;
    gap: 0.45rem;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    margin-top: 0.65rem;
    padding: 0.75rem;
  }

  .diagnostics-card > div {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    min-width: 0;
  }

  .diagnostics-card span {
    color: var(--color-text-muted);
    font-size: 0.74rem;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .diagnostics-card strong {
    color: var(--color-error);
    font-size: 0.82rem;
  }

  .diagnostics-card .ok strong {
    color: var(--color-success);
  }

  .diagnostics-card .diag-path {
    grid-column: 1 / -1;
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

  @media (max-width: 760px) {
    .download-row {
      grid-template-columns: 1fr;
    }
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

  .npu-warning {
    background: color-mix(in srgb, var(--color-warning, #d97706) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--color-warning, #d97706) 35%, var(--color-border));
    border-radius: 8px;
    font-size: 0.82rem;
    line-height: 1.6;
    margin-top: 0.75rem;
    padding: 0.6rem 0.75rem;
  }

  .npu-warning span {
    color: var(--color-text-muted);
    display: block;
    margin-top: 0.25rem;
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

  .version-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.85rem;
  }
  .version-label {
    color: var(--color-text-muted);
  }
  .version-value {
    font-family: monospace;
    font-weight: 600;
  }

  .update-available {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.6rem;
    margin-top: 0.75rem;
  }
  .update-notes {
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.6rem 0.75rem;
    font-size: 0.78rem;
    line-height: 1.5;
    color: var(--color-text-muted);
    max-height: 200px;
    overflow-y: auto;
    white-space: pre-wrap;
    align-self: stretch;
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
