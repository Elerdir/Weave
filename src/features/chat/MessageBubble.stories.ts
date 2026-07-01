import type { Meta, StoryObj } from "@storybook/svelte-vite";
import MessageBubble from "./MessageBubble.svelte";
import type { Message } from "$lib/stores/conversations.svelte";

const base: Omit<Message, "role" | "content"> = {
  id: "story-1",
  conversation_id: "conv-1",
  attachments: [],
  stats: null,
  created_at: new Date().toISOString(),
};

const meta = {
  title: "Chat/MessageBubble",
  component: MessageBubble,
  tags: ["autodocs"],
  parameters: { layout: "padded" },
} satisfies Meta<typeof MessageBubble>;

export default meta;
type Story = StoryObj<typeof meta>;

export const UserMessage: Story = {
  args: {
    msg: { ...base, role: "user", content: "Ahoj, napiš mi krátkou básničku o moři." },
  },
};

export const AssistantMessage: Story = {
  args: {
    msg: {
      ...base,
      role: "assistant",
      content: "Moře šumí, vlny hrají,\nracci nad ním křídly mávají.",
    },
  },
};

export const WithStats: Story = {
  args: {
    msg: {
      ...base,
      role: "assistant",
      content: "Tohle je odpověď se statistikami generování.",
      stats: {
        tokens_per_second: 87.3,
        prompt_tokens: 24,
        completion_tokens: 58,
        model_id: "mistral-large-latest",
        backend: "mistral_api",
      },
    },
  },
};

export const WithCodeBlock: Story = {
  args: {
    msg: {
      ...base,
      role: "assistant",
      content:
        "Tady je příklad v Rustu:\n\n```rust\nfn main() {\n    println!(\"Ahoj, Weave!\");\n}\n```\n\nA `inline kód` uvnitř věty.",
    },
  },
};
