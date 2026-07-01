import type { StorybookConfig } from "@storybook/svelte-vite";
import { mergeConfig } from "vite";
import { fileURLToPath } from "node:url";

const config: StorybookConfig = {
  stories: ["../src/**/*.stories.@(ts|svelte)"],
  framework: {
    name: "@storybook/svelte-vite",
    options: {},
  },
  async viteFinal(base) {
    return mergeConfig(base, {
      resolve: {
        alias: {
          $lib: fileURLToPath(new URL("../src/lib", import.meta.url)),
          $features: fileURLToPath(new URL("../src/features", import.meta.url)),
          $components: fileURLToPath(new URL("../src/components", import.meta.url)),
        },
      },
    });
  },
};

export default config;
