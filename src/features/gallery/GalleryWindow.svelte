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
  }

  let images = $state<GalleryImage[]>([]);
  let loading = $state(true);

  async function load() {
    loading = true;
    try {
      images = await invoke<GalleryImage[]>("list_gallery_images");
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
  });

  async function saveImage(img: GalleryImage) {
    const dest = await saveDialog({
      defaultPath: img.file_name,
      filters: [{ name: "Obrázek", extensions: ["png", "jpg", "jpeg", "webp"] }],
    });
    if (dest) await invoke("save_file_copy", { source: img.path, dest });
  }

  /** Pošle cestu hlavnímu oknu — tam se přiloží jako reference k dalšímu vstupu. */
  async function useAsReference(img: GalleryImage) {
    await emitTo("main", "use-reference", img.path);
  }

  async function deleteImage(img: GalleryImage) {
    if (!confirm(i18n.t("gallery.deleteConfirm", { name: img.file_name }))) return;
    await invoke("delete_gallery_image", { fileName: img.file_name });
    images = images.filter((i) => i.path !== img.path);
  }

  async function copyPrompt(prompt: string) {
    await navigator.clipboard.writeText(prompt);
  }

  /** Otevře obrázek v systémovém prohlížeči fotek (výchozí aplikace pro PNG). */
  async function openInViewer(img: GalleryImage) {
    try {
      await invoke("open_image_external", { path: img.path });
    } catch (e) {
      console.warn("Otevření obrázku selhalo:", e);
    }
  }

  function formatSize(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${Math.ceil(bytes / 1024)} kB`;
  }
</script>

<div class="gallery-window">
  <header>
    <h2>{i18n.m.gallery.title}</h2>
    <button class="refresh" onclick={load} disabled={loading}>{i18n.m.gallery.refresh}</button>
  </header>

  {#if images.length === 0 && !loading}
    <p class="empty">{i18n.m.gallery.empty}</p>
  {:else}
    <div class="grid">
      {#each images as img (img.path)}
        <div class="card">
          <button
            class="img-btn"
            onclick={() => openInViewer(img)}
            title={i18n.m.gallery.openExternal}
            aria-label={i18n.m.gallery.openExternal}
          >
            <img src={convertFileSrc(img.path)} alt={img.file_name} loading="lazy" />
          </button>
          <div class="card-footer">
            <span class="name" title={img.file_name}>{img.file_name}</span>
            <span class="size">{formatSize(img.size_bytes)}</span>
          </div>
          {#if img.prompt}
            <div class="prompt-box">
              <div class="prompt-head">
                <span class="prompt-label">{i18n.m.gallery.prompt}</span>
                <button
                  class="prompt-copy"
                  onclick={() => copyPrompt(img.prompt ?? "")}
                  title={i18n.m.gallery.copyPrompt}
                  aria-label={i18n.m.gallery.copyPrompt}
                >⎘</button>
              </div>
              <p class="prompt-text">{img.prompt}</p>
              {#if img.negative_prompt}
                <p class="prompt-neg" title={img.negative_prompt}>
                  <span class="prompt-label">{i18n.m.gallery.negativePrompt}</span>
                  {img.negative_prompt}
                </p>
              {/if}
            </div>
          {/if}
          <div class="card-actions">
            <button onclick={() => saveImage(img)} title={i18n.m.chat.saveImage}>💾</button>
            <button onclick={() => useAsReference(img)} title={i18n.m.chat.useAsReference}>🖼️</button>
            <button onclick={() => deleteImage(img)} title={i18n.m.gallery.delete}>🗑</button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .gallery-window {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
    height: 100vh;
    padding: 1rem 1.25rem;
    background: var(--color-bg);
    color: var(--color-text);
    box-sizing: border-box;
    overflow: hidden;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  h2 {
    margin: 0;
    font-size: 1.1rem;
  }

  .refresh {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.3rem 0.8rem;
    font-size: 0.82rem;
    cursor: pointer;
  }
  .refresh:hover:not(:disabled) {
    border-color: var(--color-accent);
  }

  .empty {
    color: var(--color-text-muted);
    text-align: center;
    margin-top: 3rem;
  }

  .grid {
    flex: 1;
    overflow-y: auto;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 0.9rem;
    align-content: start;
    padding-bottom: 1rem;
  }

  .card {
    position: relative;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 10px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .img-btn {
    display: block;
    width: 100%;
    padding: 0;
    border: none;
    background: none;
    cursor: pointer;
    line-height: 0;
  }

  .card img {
    width: 100%;
    aspect-ratio: 1;
    object-fit: cover;
    display: block;
  }

  .card-footer {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.4rem 0.6rem;
    font-size: 0.72rem;
    color: var(--color-text-muted);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .size {
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  .prompt-box {
    padding: 0 0.6rem 0.55rem;
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
  }

  .prompt-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .prompt-label {
    font-size: 0.62rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--color-text-muted);
    font-weight: 600;
  }

  .prompt-copy {
    background: transparent;
    border: none;
    color: var(--color-text-muted);
    cursor: pointer;
    font-size: 0.8rem;
    padding: 0 0.15rem;
    line-height: 1;
  }
  .prompt-copy:hover {
    color: var(--color-text);
  }

  .prompt-text {
    margin: 0;
    font-size: 0.72rem;
    line-height: 1.45;
    color: var(--color-text);
    max-height: 4.5em;
    overflow-y: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .prompt-neg {
    margin: 0.1rem 0 0;
    font-size: 0.68rem;
    line-height: 1.4;
    color: var(--color-text-muted);
    max-height: 3em;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .prompt-neg .prompt-label {
    margin-right: 0.3rem;
  }

  .card-actions {
    position: absolute;
    top: 6px;
    right: 6px;
    display: flex;
    gap: 0.25rem;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .card:hover .card-actions {
    opacity: 1;
  }

  .card-actions button {
    background: rgba(0, 0, 0, 0.55);
    border: none;
    border-radius: 6px;
    padding: 0.25rem 0.4rem;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .card-actions button:hover {
    background: rgba(0, 0, 0, 0.8);
  }
</style>
