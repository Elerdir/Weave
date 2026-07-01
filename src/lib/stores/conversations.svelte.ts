import { invoke } from "@tauri-apps/api/core";

export interface Conversation {
  id: string;
  title: string;
  persona_id: string | null;
  pinned: boolean;
  created_at: string;
  updated_at: string;
}

export interface Message {
  id: string;
  conversation_id: string;
  role: "user" | "assistant" | "system";
  content: string;
  attachments: Attachment[];
  stats: GenerationStats | null;
  created_at: string;
}

export interface Attachment {
  type: "image" | "document";
  path: string;
  mime: string;
  name?: string;
}

export interface GenerationStats {
  tokens_per_second: number;
  prompt_tokens: number;
  completion_tokens: number;
  model_id: string;
  backend: string;
}

function createConversationStore() {
  let conversations = $state<Conversation[]>([]);
  let activeId = $state<string | null>(null);
  let messages = $state<Message[]>([]);
  let loading = $state(false);
  let streamingContent = $state<string | null>(null);
  let currentStats = $state<GenerationStats | null>(null);

  return {
    get conversations() { return conversations; },
    get activeId() { return activeId; },
    get messages() { return messages; },
    get loading() { return loading; },
    get streamingContent() { return streamingContent; },
    get currentStats() { return currentStats; },

    get activeConversation() {
      return conversations.find(c => c.id === activeId) ?? null;
    },

    async loadAll() {
      conversations = await invoke<Conversation[]>("list_conversations");
    },

    async create(title: string) {
      const conv = await invoke<Conversation>("create_conversation", { title });
      conversations = [conv, ...conversations];
      activeId = conv.id;
      messages = [];
      return conv;
    },

    async select(id: string) {
      activeId = id;
      messages = await invoke<Message[]>("list_messages", { conversationId: id });
    },

    async setActivePersona(personaId: string | null) {
      if (!activeId) return;
      await invoke("set_conversation_persona", { conversationId: activeId, personaId });
      conversations = conversations.map((c) =>
        c.id === activeId ? { ...c, persona_id: personaId } : c
      );
    },

    async delete(id: string) {
      await invoke("delete_conversation", { id });
      conversations = conversations.filter(c => c.id !== id);
      if (activeId === id) {
        activeId = conversations[0]?.id ?? null;
        messages = activeId
          ? await invoke<Message[]>("list_messages", { conversationId: activeId })
          : [];
      }
    },

    async rename(id: string, title: string) {
      const trimmed = title.trim();
      if (!trimmed) return;
      await invoke("rename_conversation", { id, title: trimmed });
      conversations = conversations.map((c) =>
        c.id === id ? { ...c, title: trimmed } : c
      );
    },

    async togglePin(id: string) {
      const conv = conversations.find((c) => c.id === id);
      if (!conv) return;
      const pinned = !conv.pinned;
      await invoke("set_conversation_pinned", { id, pinned });
      // Aktualizuj + přeřaď: připnuté nahoře, pak dle updated_at
      conversations = conversations
        .map((c) => (c.id === id ? { ...c, pinned } : c))
        .sort((a, b) => {
          if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
          return b.updated_at.localeCompare(a.updated_at);
        });
    },

    appendStreamToken(token: string) {
      streamingContent = (streamingContent ?? "") + token;
    },

    finalizeStream(stats: GenerationStats) {
      if (streamingContent !== null) {
        const assistantMsg: Message = {
          id: crypto.randomUUID(),
          conversation_id: activeId!,
          role: "assistant",
          content: streamingContent,
          attachments: [],
          stats,
          created_at: new Date().toISOString(),
        };
        messages = [...messages, assistantMsg];
      }
      streamingContent = null;
      currentStats = stats;
      loading = false;
    },

    startLoading() {
      loading = true;
      streamingContent = null;
      currentStats = null;
    },

    pushUserMessage(content: string) {
      const msg: Message = {
        id: crypto.randomUUID(),
        conversation_id: activeId!,
        role: "user",
        content,
        attachments: [],
        stats: null,
        created_at: new Date().toISOString(),
      };
      messages = [...messages, msg];
    },
  };
}

export const conversationStore = createConversationStore();
