/**
 * Fronta obrázků čekajících na přiložení jako reference — most mezi
 * MessageBubble („Použít jako referenci“ u vygenerovaného obrázku)
 * a ChatView (vlastní seznam příloh u vstupu).
 */
function createReferenceQueue() {
  let queue = $state<string[]>([]);

  return {
    get pending() {
      return queue;
    },
    add(path: string) {
      queue = [...queue, path];
    },
    /** Vrátí a vyprázdní frontu (volá ChatView). */
    drain(): string[] {
      const items = queue;
      queue = [];
      return items;
    },
  };
}

export const referenceQueue = createReferenceQueue();
