import { invoke } from "@tauri-apps/api/core";

export interface Persona {
  id: string;
  name: string;
  icon: string;
  system_prompt: string;
  builtin: boolean;
}

function createPersonaStore() {
  let personas = $state<Persona[]>([]);

  return {
    get personas() {
      return personas;
    },

    byId(id: string | null): Persona | null {
      if (!id) return null;
      return personas.find((p) => p.id === id) ?? null;
    },

    async load() {
      personas = await invoke<Persona[]>("list_personas");
    },

    async create(name: string, icon: string, systemPrompt: string) {
      const p = await invoke<Persona>("create_persona", {
        name: name.trim(),
        icon: icon.trim() || "🎭",
        systemPrompt: systemPrompt.trim(),
      });
      personas = [...personas, p];
      return p;
    },

    async remove(id: string) {
      await invoke("delete_persona", { id });
      personas = personas.filter((p) => p.id !== id);
    },
  };
}

export const personaStore = createPersonaStore();
