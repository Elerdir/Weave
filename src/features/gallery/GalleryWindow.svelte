<script lang="ts">
  import { onMount } from "svelte";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { emitTo } from "@tauri-apps/api/event";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";
  import { i18n } from "$lib/i18n/index.svelte";

  type SortMode = "newest" | "oldest" | "name" | "size" | "favorites";

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

  const FAVORITES_KEY = "weave.gallery.favorites.v1";

  let images = $state<GalleryImage[]>([]);
  let loading = $state(true);
  let selected = $state<GalleryImage | null>(null);
  let query = $state("");
  let sortMode = $state<SortMode>("newest");
  let favoritesOnly = $state(false);
  let tileSize = $state(190);
  let favoritePaths = $state<Set<string>>(new Set());

  const visibleImages = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return [...images]
      .filter((img) => {
        if (favoritesOnly && !favoritePaths.has(img.path)) return false;
        if (!q) return true;
        return [
          img.file_name,
          img.prompt ?? "",
          img.negative_prompt ?? "",
          img.original_prompt ?? "",
          img.reference_preservation ?? "",
        ].some((value) => value.toLowerCase().includes(q));
      })
      .sort((a, b) => {
        if (sortMode === "favorites") {
          const fav = Number(favoritePaths.has(b.path)) - Number(favoritePaths.has(a.path));
          if (fav !== 0) return fav;
          return b.modified_at - a.modified_at;
        }
        if (sortMode === "oldest") return a.modified_at - b.modified_at;
        if (sortMode === "name") return a.file_name.localeCompare(b.file_name);
        if (sortMode === "size") return b.size_bytes - a.size_bytes;
        return b.modified_at - a.modified_at;
      });
  });

  async function load() {
    loading = true;
    try {
      images = await invoke<GalleryImage[]>("list_gallery_images");
      if (selected && !images.some((img) => img.path === selected?.path)) {
        selected = null;
      }
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    try {
      const raw = localStorage.getItem(FAVORITES_KEY);
      favoritePaths = new Set(raw ? JSON.parse(raw) : []);
    } catch {
      favoritePaths = new Set();
    }
    void load();
  });

  function persistFavorites(next: Set<string>) {
    favoritePaths = next;
    localStorage.setItem(FAVORITES_KEY, JSON.stringify([...next]));
  }

  function toggleFavorite(img: GalleryImage) {
    const next = new Set(favoritePaths);
    if (next.has(img.path)) next.delete(img.path);
    else next.add(img.path);
    persistFavorites(next);
  }

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
    images = images.filter((i) => i.path !== img.path);
    if (favoritePaths.has(img.path)) {
      const next = new Set(favoritePaths);
      next.delete(img.path);
      persistFavorites(next);
    }
    if (selected?.path === img.path) selected = null;
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
    if (dest) {
      await invoke("export_gallery_image_metadata", { fileName: img.file_name, dest });
    }
  }

  async function openInViewer(img: GalleryImage) {
    try {
      await invoke("open_image_external", { path: img.path });
    } catch (e) {
      console.warn("Opening image failed:", e);
    }
  }

  async function openDetailWindow(img: GalleryImage) {
    try {
      await invoke("open_gallery_detail_window", { path: img.path });
    } catch (e) {
      console.warn("Opening image detail failed:", e);
      selected = img;
    }
  }

  function formatSize(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${Math.ceil(bytes / 1024)} kB`;
  }

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleString();
  }

  function closeDetail() {
    selected = null;
  }

  function selectRelative(delta: number) {
    if (!selected || visibleImages.length === 0) return;
    const idx = visibleImages.findIndex((img) => img.path === selected?.path);
    const current = idx >= 0 ? idx : delta >= 0 ? -1 : 0;
    const next = (current + delta + visibleImages.length) % visibleImages.length;
    selected = visibleImages[next];
  }

  function onBackdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) closeDetail();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") closeDetail();
    if (!selected) return;
    if (e.key === "ArrowLeft") selectRelative(-1);
    if (e.key === "ArrowRight") selectRelative(1);
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="gallery-window">
  <header>
    <h2>{i18n.m.gallery.title}</h2>
    <div class="header-actions">
      <span class="count">{visibleImages.length} / {images.length}</span>
      <button class="refresh" onclick={load} disabled={loading}>{i18n.m.gallery.refresh}</button>
    </div>
  </header>

  <section class="toolbar" aria-label={i18n.m.gallery.tools}>
    <input
      class="search"
      placeholder={i18n.m.gallery.searchPlaceholder}
      bind:value={query}
      aria-label={i18n.m.gallery.searchPlaceholder}
    />
    <select bind:value={sortMode} aria-label={i18n.m.gallery.sort}>
      <option value="newest">{i18n.m.gallery.sortNewest}</option>
      <option value="oldest">{i18n.m.gallery.sortOldest}</option>
      <option value="name">{i18n.m.gallery.sortName}</option>
      <option value="size">{i18n.m.gallery.sortSize}</option>
      <option value="favorites">{i18n.m.gallery.sortFavorites}</option>
    </select>
    <label class="fav-filter">
      <input type="checkbox" bind:checked={favoritesOnly} />
      <span>{i18n.m.gallery.favoritesOnly}</span>
    </label>
    <label class="tile-size">
      <span>{i18n.m.gallery.thumbnailSize}</span>
      <input type="range" min="130" max="320" step="10" bind:value={tileSize} />
    </label>
  </section>

  {#if images.length === 0 && !loading}
    <p class="empty">{i18n.m.gallery.empty}</p>
  {:else if visibleImages.length === 0 && !loading}
    <p class="empty">{i18n.m.gallery.noMatches}</p>
  {:else}
    <div class="grid" style={`--tile-min: ${tileSize}px`}>
      {#each visibleImages as img (img.path)}
        <article class="card" class:favorite={favoritePaths.has(img.path)}>
          <button
            class="img-btn"
            onclick={() => openDetailWindow(img)}
            title={i18n.m.gallery.openDetail}
            aria-label={i18n.m.gallery.openDetail}
          >
            <img src={convertFileSrc(img.path)} alt={img.file_name} loading="lazy" />
          </button>
          <button
            class="favorite-btn"
            class:active={favoritePaths.has(img.path)}
            onclick={() => toggleFavorite(img)}
            title={i18n.m.gallery.favorite}
            aria-label={i18n.m.gallery.favorite}
          >★</button>
          <div class="card-footer">
            <span class="name" title={img.file_name}>{img.file_name}</span>
            <span class="size">{formatSize(img.size_bytes)}</span>
          </div>
          <p class="prompt-preview" title={img.prompt ?? i18n.m.gallery.noPrompt}>
            {img.prompt ?? i18n.m.gallery.noPrompt}
          </p>
          <div class="card-actions">
            <button onclick={() => saveImage(img)} title={i18n.m.chat.saveImage}>Save</button>
            <button onclick={() => useAsReference(img)} title={i18n.m.chat.useAsReference}>Ref</button>
            <button onclick={() => usePromptInChat(img)} disabled={!img.prompt} title={i18n.m.gallery.usePrompt}>Prompt</button>
            <button onclick={() => deleteImage(img)} title={i18n.m.gallery.delete}>Delete</button>
          </div>
        </article>
      {/each}
    </div>
  {/if}
</div>

{#if selected}
  <div class="detail-backdrop" role="presentation" onclick={onBackdropClick}>
    <div class="detail-modal" role="dialog" aria-modal="true" aria-label={i18n.m.gallery.details}>
      <header class="detail-header">
        <div>
          <h3>{i18n.m.gallery.details}</h3>
          <p>{selected.file_name}</p>
        </div>
        <div class="detail-nav">
          <button onclick={() => selectRelative(-1)} aria-label={i18n.m.gallery.previous}>‹</button>
          <button onclick={() => selectRelative(1)} aria-label={i18n.m.gallery.next}>›</button>
          <button class="detail-close" onclick={closeDetail} aria-label={i18n.m.common.cancel}>x</button>
        </div>
      </header>

      <div class="detail-body">
        <div class="detail-image">
          <img src={convertFileSrc(selected.path)} alt={selected.file_name} />
        </div>

        <aside class="detail-info">
          <dl class="meta-list">
            <div>
              <dt>{i18n.m.gallery.file}</dt>
              <dd title={selected.path}>{selected.file_name}</dd>
            </div>
            <div>
              <dt>{i18n.m.gallery.size}</dt>
              <dd>{formatSize(selected.size_bytes)}</dd>
            </div>
            <div>
              <dt>{i18n.m.gallery.modified}</dt>
              <dd>{formatDate(selected.modified_at)}</dd>
            </div>
            <div>
              <dt>{i18n.m.gallery.aiStamp}</dt>
              <dd>{selected.ai_stamped ? i18n.m.common.yes : i18n.m.common.no}</dd>
            </div>
            <div>
              <dt>{i18n.m.gallery.path}</dt>
              <dd title={selected.path}>{selected.path}</dd>
            </div>
          </dl>

          <section class="prompt-detail">
            <div class="prompt-detail-head">
              <h4>{i18n.m.gallery.originalPrompt}</h4>
              <button
                onclick={() => copyPrompt(selected?.original_prompt ?? null)}
                disabled={!selected.original_prompt}
              >
                {i18n.m.gallery.copyPrompt}
              </button>
            </div>
            <p>{selected.original_prompt ?? i18n.m.gallery.noPrompt}</p>
          </section>

          <section class="prompt-detail">
            <div class="prompt-detail-head">
              <h4>{i18n.m.gallery.prompt}</h4>
              <button onclick={() => copyPrompt(selected?.prompt ?? null)} disabled={!selected.prompt}>
                {i18n.m.gallery.copyPrompt}
              </button>
            </div>
            <p>{selected.prompt ?? i18n.m.gallery.noPrompt}</p>
          </section>

          <section class="prompt-detail">
            <div class="prompt-detail-head">
              <h4>{i18n.m.gallery.negativePrompt}</h4>
              <button
                onclick={() => copyPrompt(selected?.negative_prompt ?? null)}
                disabled={!selected.negative_prompt}
              >
                {i18n.m.gallery.copyPrompt}
              </button>
            </div>
            <p>{selected.negative_prompt ?? i18n.m.gallery.noPrompt}</p>
          </section>

          <section class="prompt-detail">
            <div class="prompt-detail-head">
              <h4>{i18n.m.gallery.referencePreservation}</h4>
              <button
                onclick={() => copyPrompt(selected?.reference_preservation ?? null)}
                disabled={!selected.reference_preservation}
              >
                {i18n.m.gallery.copyPrompt}
              </button>
            </div>
            <p>{selected.reference_preservation ?? i18n.m.gallery.noPrompt}</p>
          </section>

          <div class="detail-actions">
            <button onclick={() => saveImage(selected!)}>{i18n.m.chat.saveImage}</button>
            <button onclick={() => useAsReference(selected!)}>{i18n.m.chat.useAsReference}</button>
            <button onclick={() => usePromptInChat(selected!)} disabled={!selected.prompt}>{i18n.m.gallery.usePrompt}</button>
            <button onclick={() => copyAllMetadata(selected!)}>{i18n.m.gallery.copyMetadata}</button>
            <button onclick={() => exportMetadata(selected!)}>{i18n.m.gallery.exportMetadata}</button>
            <button onclick={() => openInViewer(selected!)}>{i18n.m.gallery.openExternal}</button>
            <button class="danger" onclick={() => deleteImage(selected!)}>{i18n.m.gallery.delete}</button>
          </div>
        </aside>
      </div>
    </div>
  </div>
{/if}

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
    gap: 1rem;
  }

  .header-actions,
  .detail-nav {
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }

  .count {
    color: var(--color-text-muted);
    font-size: 0.78rem;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  h2, h3, h4, p {
    margin: 0;
  }

  h2 {
    font-size: 1.1rem;
  }

  .refresh,
  .detail-actions button,
  .prompt-detail button,
  .detail-nav button,
  .detail-close {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.35rem 0.7rem;
    font-size: 0.78rem;
    cursor: pointer;
  }

  .refresh:hover:not(:disabled),
  .detail-actions button:hover:not(:disabled),
  .prompt-detail button:hover:not(:disabled),
  .detail-nav button:hover,
  .detail-close:hover {
    border-color: var(--color-accent);
  }

  button:disabled {
    cursor: default;
    opacity: 0.45;
  }

  .toolbar {
    display: grid;
    grid-template-columns: minmax(180px, 1fr) minmax(130px, auto) auto minmax(160px, 220px);
    gap: 0.55rem;
    align-items: center;
  }

  .search,
  .toolbar select {
    min-width: 0;
    background: var(--color-surface);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    padding: 0.42rem 0.55rem;
    font-size: 0.82rem;
  }

  .search:focus,
  .toolbar select:focus {
    outline: none;
    border-color: var(--color-accent);
  }

  .fav-filter,
  .tile-size {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--color-text-muted);
    font-size: 0.78rem;
    white-space: nowrap;
  }

  .tile-size input {
    width: 100%;
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
    grid-template-columns: repeat(auto-fill, minmax(var(--tile-min, 190px), 1fr));
    gap: 0.75rem;
    align-content: start;
    padding: 0 0.15rem 1rem 0;
  }

  .card {
    position: relative;
    background: var(--color-surface);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .card.favorite {
    border-color: color-mix(in srgb, #d29922 55%, var(--color-border));
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
    aspect-ratio: 4 / 3;
    object-fit: cover;
    display: block;
  }

  .favorite-btn {
    position: absolute;
    top: 6px;
    left: 6px;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: 6px;
    background: rgba(0, 0, 0, 0.56);
    color: rgba(255, 255, 255, 0.72);
    cursor: pointer;
    font-size: 0.88rem;
    line-height: 1;
  }

  .favorite-btn.active {
    background: color-mix(in srgb, #d29922 82%, black);
    color: white;
  }

  .card-footer {
    display: flex;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.45rem 0.55rem 0.15rem;
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

  .prompt-preview {
    padding: 0 0.55rem 0.55rem;
    font-size: 0.72rem;
    line-height: 1.35;
    color: var(--color-text);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    min-height: 2.1rem;
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

  .card:hover .card-actions,
  .card:focus-within .card-actions {
    opacity: 1;
  }

  .card-actions button {
    background: rgba(0, 0, 0, 0.62);
    color: white;
    border: none;
    border-radius: 5px;
    padding: 0.25rem 0.4rem;
    font-size: 0.68rem;
    cursor: pointer;
  }

  .card-actions button:disabled {
    display: none;
  }

  .detail-backdrop {
    position: fixed;
    inset: 0;
    z-index: 100;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1rem;
  }

  .detail-modal {
    width: min(1180px, 96vw);
    max-height: 94vh;
    background: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    box-shadow: 0 24px 80px rgba(0, 0, 0, 0.35);
  }

  .detail-header {
    padding: 0.85rem 1rem;
    border-bottom: 1px solid var(--color-border);
  }

  .detail-header h3 {
    font-size: 1rem;
  }

  .detail-header p {
    margin-top: 0.2rem;
    font-size: 0.78rem;
    color: var(--color-text-muted);
  }

  .detail-close {
    padding: 0.25rem 0.55rem;
    font-size: 0.9rem;
  }

  .detail-body {
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(280px, 360px);
    gap: 1rem;
    padding: 1rem;
    overflow: auto;
  }

  .detail-image {
    min-width: 0;
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
    max-height: min(76vh, 820px);
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

  .meta-list dt,
  .prompt-detail h4 {
    font-size: 0.68rem;
    text-transform: uppercase;
    color: var(--color-text-muted);
    font-weight: 700;
    letter-spacing: 0.02em;
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
    color: var(--color-text);
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

  @media (max-width: 760px) {
    .gallery-window {
      padding: 0.8rem;
    }

    .toolbar {
      grid-template-columns: 1fr;
      align-items: stretch;
    }

    .fav-filter,
    .tile-size {
      justify-content: space-between;
    }

    .detail-body {
      grid-template-columns: 1fr;
    }

    .detail-image img {
      max-height: 52vh;
    }
  }
</style>
