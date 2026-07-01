import "../src/app.css";
import type { Preview } from "@storybook/svelte-vite";

const preview: Preview = {
  parameters: {
    controls: {
      matchers: { color: /(background|color)$/i, date: /Date$/i },
    },
    backgrounds: {
      default: "weave-dark",
      values: [
        { name: "weave-dark", value: "#0f0f13" },
        { name: "weave-light", value: "#f4f4f8" },
      ],
    },
  },
};

export default preview;
