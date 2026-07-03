/**
 * Obrázek čekající na úpravu (img2img) — most mezi MessageBubble
 * („Upravit obrázek" u vygenerovaného obrázku) a ChatView (badge nad
 * vstupem + odeslání instrukce jako edit_image_message).
 */
function createEditImageStore() {
  let pending = $state<string | null>(null);

  return {
    get pending() {
      return pending;
    },
    set(path: string) {
      pending = path;
    },
    clear() {
      pending = null;
    },
    /** Vrátí a vyčistí čekající obrázek (volá ChatView při odeslání). */
    take(): string | null {
      const path = pending;
      pending = null;
      return path;
    },
  };
}

export const editImageStore = createEditImageStore();
