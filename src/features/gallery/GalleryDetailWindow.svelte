<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { emitTo } from "@tauri-apps/api/event";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { i18n } from "$lib/i18n/index.svelte";

  interface GalleryImage {
    path: string;
    file_name: string;
    size_bytes: number;
    modified_at: number;
    prompt: string | null;
    negative_prompt: string | null;
    original_prompt: string | null;
    reference_preservation: string | null;
    ai_stamped: boolean;
  }

  const path = new URLSearchParams(window.location.search).get("path");
  let image = $state<GalleryImage | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    if (!path) {
      error = i18n.m.gallery.noMatches;
      loading = false;
      return;
    }

    try {
      const images = await invoke<GalleryImage[]>("list_gallery_images");
      image = images.find((img) => img.path === path) ?? null;
      if (!image) error = i18n.m.gallery.noMatches;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  });

  async function saveImage(img: GalleryImage) {
    const dest = await saveDialog({
      defaultPath: img.file_name,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (dest) await invoke("save_file_copy", { source: img.path, dest });
  }

  async function useAsReference(img: GalleryImage) {
    await emitTo("main", "use-reference", img.path);
  }

  async function usePromptInChat(img: GalleryImage) {
    if (!img.prompt) return;
    await emitTo("main", "use-prompt", img.prompt);
  }

  async function deleteImage(img: GalleryImage) {
    if (!confirm(i18n.t("gallery.deleteConfirm", { name: img.file_name }))) return;
    await invoke("delete_gallery_image", { fileName: img.file_name });
    window.close();
  }

  async function copyPrompt(prompt: string | null) {
    if (prompt) await navigator.clipboard.writeText(prompt);
  }

  async function copyAllMetadata(img: GalleryImage) {
    await navigator.clipboard.writeText(JSON.stringify(img, null, 2));
  }

  async function exportMetadata(img: GalleryImage) {
    const base = img.file_name.replace(/\.[^.]+$/, "");
    const dest = await saveDialog({
      defaultPath: `${base}.metadata.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (dest) await invoke("export_gallery_image_metadata", { fileName: img.file_name, dest });
  }

  async function openInViewer(img: GalleryImage) {
    await invoke("open_image_external", { path: img.path });
  }

  function formatSize(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${Math.ceil(bytes / 1024)} kB`;
  }

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleString();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") window.close();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="detail-window">
  <header class="detail-header">
    <div>
      <h2>{i18n.m.gallery.details}</h2>
      <p>{image?.file_name ?? i18n.m.gallery.title}</p>
    </div>
    <button class="close" onclick={() => window.close()} aria-label={i18n.m.common.cancel}>x</button>
  </header>

  {#if loading}
    <p class="empty">{i18n.m.common.loading}</p>
  {:else if error || !image}
    <p class="empty">{error ?? i18n.m.gallery.noMatches}</p>
  {:else}
    <main class="detail-body">
      <section class="detail-image">
        <img src={convertFileSrc(image.path)} alt={image.file_name} />
      </section>

      <aside class="detail-info">
        <dl class="meta-list">
          <div>
            <dt>{i18n.m.gallery.file}</dt>
            <dd title={image.path}>{image.file_name}</dd>
          </div>
          <div>
            <dt>{i18n.m.gallery.size}</dt>
            <dd>{formatSize(image.size_bytes)}</dd>
          </div>
          <div>
            <dt>{i18n.m.gallery.modified}</dt>
            <dd>{formatDate(image.modified_at)}</dd>
          </div>
          <div>
            <dt>{i18n.m.gallery.aiStamp}</dt>
            <dd>{image.ai_stamped ? i18n.m.common.yes : i18n.m.common.no}</dd>
          </div>
          <div>
            <dt>{i18n.m.gallery.path}</dt>
            <dd title={image.path}>{image.path}</dd>
          </div>
        </dl>

        <section class="prompt-detail">
          <div class="prompt-detail-head">
            <h3>{i18n.m.gallery.originalPrompt}</h3>
            <button onclick={() => copyPrompt(image?.original_prompt ?? null)} disabled={!image.original_prompt}>
              {i18n.m.gallery.copyPrompt}
            </button>
          </div>
          <p>{image.original_prompt ?? i18n.m.gallery.noPrompt}</p>
        </section>

        <section class="prompt-detail">
          <div class="prompt-detail-head">
            <h3>{i18n.m.gallery.prompt}</h3>
            <button onclick={() => copyPrompt(image?.prompt ?? null)} disabled={!image.prompt}>
              {i18n.m.gallery.copyPrompt}
            </button>
          </div>
          <p>{image.prompt ?? i18n.m.gallery.noPrompt}</p>
        </section>

        <section class="prompt-detail">
          <div class="prompt-detail-head">
            <h3>{i18n.m.gallery.negativePrompt}</h3>
            <button onclick={() => copyPrompt(image?.negative_prompt ?? null)} disabled={!image.negative_prompt}>
              {i18n.m.gallery.copyPrompt}
            </button>
          </div>
          <p>{image.negative_prompt ?? i18n.m.gallery.noPrompt}</p>
        </section>

        <section class="prompt-detail">
          <div class="prompt-detail-head">
            <h3>{i18n.m.gallery.referencePreservation}</h3>
            <button
              onclick={() => copyPrompt(image?.reference_preservation ?? null)}
              disabled={!image.reference_preservation}
            >
              {i18n.m.gallery.copyPrompt}
            </button>
          </div>
          <p>{image.reference_preservation ?? i18n.m.gallery.noPrompt}</p>
        </section>

        <div class="detail-actions">
          <button onclick={() => saveImage(image!)}>{i18n.m.chat.saveImage}</button>
          <button onclick={() => useAsReference(image!)}>{i18n.m.chat.useAsReference}</button>
          <button onclick={() => usePromptInChat(image!)} disabled={!image.prompt}>{i18n.m.gallery.usePrompt}</button>
          <button onclick={() => copyAllMetadata(image!)}>{i18n.m.gallery.copyMetadata}</button>
          <button onclick={() => exportMetadata(image!)}>{i18n.m.gallery.exportMetadata}</button>
          <button onclick={() => openInViewer(image!)}>{i18n.m.gallery.openExternal}</button>
          <button class="danger" onclick={() => deleteImage(image!)}>{i18n.m.gallery.delete}</button>
        </div>
      </aside>
    </main>
  {/if}
</div>

<style>
  .detail-window {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--color-bg);
    color: var(--color-text);
    overflow: hidden;
  }

  .detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.85rem 1rem;
    border-bottom: 1px solid var(--color-border);
    background: var(--color-surface);
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  h2 {
    font-size: 1rem;
  }

  h3 {
    font-size: 0.72rem;
    text-transform: uppercase;
    color: var(--color-text-muted);
    font-weight: 700;
  }

  .detail-header p {
    margin-top: 0.2rem;
    font-size: 0.78rem;
    color: var(--color-text-muted);
  }

  .close,
  .detail-actions button,
  .prompt-detail button {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.35rem 0.7rem;
    font-size: 0.78rem;
    cursor: pointer;
  }

  button:disabled {
    cursor: default;
    opacity: 0.45;
  }

  .empty {
    color: var(--color-text-muted);
    text-align: center;
    margin-top: 3rem;
  }

  .detail-body {
    min-height: 0;
    flex: 1;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(320px, 420px);
    gap: 1rem;
    padding: 1rem;
    overflow: auto;
  }

  .detail-image {
    min-width: 0;
    min-height: 420px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    overflow: hidden;
  }

  .detail-image img {
    max-width: 100%;
    max-height: calc(100vh - 150px);
    object-fit: contain;
    display: block;
  }

  .detail-info {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.85rem;
  }

  .meta-list {
    display: grid;
    gap: 0.55rem;
    margin: 0;
    padding: 0;
  }

  .meta-list div {
    min-width: 0;
  }

  .meta-list dt {
    font-size: 0.68rem;
    text-transform: uppercase;
    color: var(--color-text-muted);
    font-weight: 700;
  }

  .meta-list dd {
    margin: 0.15rem 0 0;
    font-size: 0.82rem;
    overflow-wrap: anywhere;
  }

  .prompt-detail {
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.75rem;
  }

  .prompt-detail-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 0.5rem;
  }

  .prompt-detail p {
    margin-top: 0.45rem;
    font-size: 0.82rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .detail-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.45rem;
  }

  .detail-actions .danger {
    color: #b91c1c;
    border-color: rgba(185, 28, 28, 0.35);
  }

  @media (max-width: 820px) {
    .detail-body {
      grid-template-columns: 1fr;
    }

    .detail-image {
      min-height: 260px;
    }

    .detail-image img {
      max-height: 58vh;
    }
  }
</style>
