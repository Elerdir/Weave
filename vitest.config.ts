import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte({ hot: !process.env.VITEST })],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test-setup.ts"],
    globals: true,
    coverage: {
      provider: "v8",
      reporter: ["text", "html", "lcov"],
      thresholds: {
        lines: 70,
        functions: 70,
        branches: 65,
      },
      include: ["src/**/*.{ts,svelte}"],
      exclude: ["src/main.ts", "src/**/*.stories.*"],
    },
  },
  resolve: {
    alias: {
      $lib: "/src/lib",
      $features: "/src/features",
      $components: "/src/components",
    },
  },
});
