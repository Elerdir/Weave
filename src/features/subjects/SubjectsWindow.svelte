<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { emitTo } from "@tauri-apps/api/event";
  import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
  import { subjectsStore, type Subject } from "$lib/stores/subjects.svelte";
  import { IMAGE_EXTENSIONS } from "$lib/reference-images";
  import { i18n } from "$lib/i18n/index.svelte";

  let newName = $state("");

  onMount(() => {
    void subjectsStore.load();
  });

  async function createSubject() {
    const name = newName.trim();
    if (!name) return;
    await subjectsStore.create(name);
    newName = "";
  }

  async function addPhotos(subject: Subject) {
    const picked = await openFilePicker({
      multiple: true,
      filters: [{ name: "Obrázky", extensions: IMAGE_EXTENSIONS }],
    });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    for (const p of paths) {
      await subjectsStore.addImage(subject.id, p);
    }
  }

  async function deleteSubject(subject: Subject) {
    if (!confirm(i18n.t("subjects.deleteConfirm", { name: subject.name }))) return;
    await subjectsStore.remove(subject.id);
  }

  /** Přiloží všechny fotky postavy jako reference do hlavního okna chatu. */
  async function useInChat(subject: Subject) {
    for (const img of subject.images) {
      await emitTo("main", "use-reference", img.path);
    }
  }
</script>

<div class="subjects-window">
  <header>
    <h2>{i18n.m.subjects.title}</h2>
    <div class="add-row">
      <input
        placeholder={i18n.m.subjects.namePlaceholder}
        bind:value={newName}
        onkeydown={(e) => e.key === "Enter" && createSubject()}
      />
      <button class="primary" onclick={createSubject} disabled={!newName.trim()}>
        {i18n.m.subjects.add}
      </button>
    </div>
  </header>

  {#if subjectsStore.subjects.length === 0 && !subjectsStore.loading}
    <p class="empty">{i18n.m.subjects.empty}</p>
  {/if}

  <div class="list">
    {#each subjectsStore.subjects as subject (subject.id)}
      <div class="subject">
        <div class="subject-head">
          <input
            class="name-input"
            value={subject.name}
            onchange={(e) => subjectsStore.rename(subject.id, (e.target as HTMLInputElement).value)}
          />
          <div class="head-actions">
            <button
              class="use-btn"
              onclick={() => useInChat(subject)}
              disabled={subject.images.length === 0}
              title={i18n.m.subjects.useInChat}
            >🖼️ {i18n.m.subjects.useInChat}</button>
            <button class="danger" onclick={() => deleteSubject(subject)} title={i18n.m.subjects.delete}>🗑</button>
          </div>
        </div>

        <textarea
          class="notes"
          placeholder={i18n.m.subjects.notesPlaceholder}
          value={subject.notes}
          onchange={(e) => subjectsStore.setNotes(subject.id, (e.target as HTMLTextAreaElement).value)}
          rows="2"
        ></textarea>

        <div class="photos">
          {#each subject.images as img (img.id)}
            <div class="photo">
              <img src={convertFileSrc(img.path)} alt="" loading="lazy" />
              <button
                class="photo-remove"
                onclick={() => subjectsStore.removeImage(subject.id, img.id)}
                aria-label={i18n.m.subjects.removePhoto}
              >×</button>
            </div>
          {/each}
          <button class="add-photo" onclick={() => addPhotos(subject)}>
            + {i18n.m.subjects.addPhotos}
          </button>
        </div>
      </div>
    {/each}
  </div>
</div>

<style>
  .subjects-window {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    height: 100vh;
    padding: 1rem 1.25rem;
    background: var(--color-bg);
    color: var(--color-text);
    box-sizing: border-box;
    overflow-y: auto;
  }

  header {
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }
  h2 {
    margin: 0;
    font-size: 1.1rem;
  }
  .add-row {
    display: flex;
    gap: 0.5rem;
  }
  .add-row input {
    flex: 1;
  }

  input,
  textarea {
    background: var(--color-surface-2);
    color: var(--color-text);
    border: 1px solid var(--color-border);
    border-radius: 8px;
    padding: 0.45rem 0.65rem;
    font-size: 0.9rem;
    font-family: inherit;
    outline: none;
  }
  input:focus,
  textarea:focus {
    border-color: var(--color-accent);
  }

  button {
    border: 1px solid var(--color-border);
    background: var(--color-surface-2);
    color: var(--color-text);
    border-radius: 8px;
    padding: 0.4rem 0.75rem;
    font-size: 0.82rem;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    border-color: var(--color-accent);
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  button.primary {
    background: var(--color-accent);
    color: #fff;
    border: none;
    font-weight: 600;
  }
  button.danger {
    color: var(--color-error);
  }
  button.danger:hover {
    border-color: var(--color-error);
  }

  .empty {
    color: var(--color-text-muted);
    text-align: center;
    margin-top: 2rem;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 1rem;
    padding-bottom: 1rem;
  }

  .subject {
    border: 1px solid var(--color-border);
    border-radius: 12px;
    padding: 0.85rem 1rem;
    background: var(--color-surface);
    display: flex;
    flex-direction: column;
    gap: 0.6rem;
  }

  .subject-head {
    display: flex;
    gap: 0.75rem;
    align-items: center;
  }
  .name-input {
    flex: 1;
    font-weight: 600;
    font-size: 0.95rem;
  }
  .head-actions {
    display: flex;
    gap: 0.4rem;
  }
  .use-btn {
    color: var(--color-accent);
    border-color: var(--color-accent);
  }

  .notes {
    resize: vertical;
    font-size: 0.82rem;
    color: var(--color-text-muted);
  }

  .photos {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
  }
  .photo {
    position: relative;
    width: 84px;
    height: 84px;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--color-border);
  }
  .photo img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .photo-remove {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 18px;
    height: 18px;
    padding: 0;
    line-height: 1;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
    border: none;
    border-radius: 50%;
    font-size: 0.8rem;
  }
  .add-photo {
    width: 84px;
    height: 84px;
    border-style: dashed;
    font-size: 0.72rem;
    color: var(--color-text-muted);
  }
</style>
