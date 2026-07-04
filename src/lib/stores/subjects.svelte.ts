import { invoke } from "@tauri-apps/api/core";

export interface SubjectImage {
  id: string;
  path: string;
  mime: string;
}

export interface Subject {
  id: string;
  name: string;
  notes: string;
  images: SubjectImage[];
}

/**
 * Referenční postavy — pojmenované osoby s uloženými fotkami, které se dají
 * jedním klikem přiložit jako reference (PuLID) při generování obrázků.
 */
function createSubjectsStore() {
  let subjects = $state<Subject[]>([]);
  let loading = $state(false);

  async function load() {
    loading = true;
    try {
      subjects = await invoke<Subject[]>("list_subjects");
    } finally {
      loading = false;
    }
  }

  return {
    get subjects() {
      return subjects;
    },
    get loading() {
      return loading;
    },
    load,

    async create(name: string): Promise<Subject> {
      const subject = await invoke<Subject>("create_subject", { name });
      subjects = [subject, ...subjects];
      return subject;
    },

    async rename(id: string, name: string) {
      await invoke("rename_subject", { id, name });
      subjects = subjects.map((s) => (s.id === id ? { ...s, name } : s));
    },

    async setNotes(id: string, notes: string) {
      await invoke("set_subject_notes", { id, notes });
      subjects = subjects.map((s) => (s.id === id ? { ...s, notes } : s));
    },

    async remove(id: string) {
      await invoke("delete_subject", { id });
      subjects = subjects.filter((s) => s.id !== id);
    },

    async addImage(subjectId: string, sourcePath: string) {
      const img = await invoke<SubjectImage>("add_subject_image", { subjectId, sourcePath });
      subjects = subjects.map((s) =>
        s.id === subjectId ? { ...s, images: [...s.images, img] } : s
      );
    },

    async removeImage(subjectId: string, imageId: string) {
      await invoke("remove_subject_image", { imageId });
      subjects = subjects.map((s) =>
        s.id === subjectId ? { ...s, images: s.images.filter((i) => i.id !== imageId) } : s
      );
    },
  };
}

export const subjectsStore = createSubjectsStore();
