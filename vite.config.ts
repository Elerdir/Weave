import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      // `target/` je v korenu workspace (ne v src-tauri), takze ho `**/src-tauri/**`
      // nepokryje. Bez nej Vite sleduje i .dll, ktere prave zapisuje linker, a
      // watcher spadne na `EBUSY: resource busy or locked` -> padá cely dev server.
      ignored: ["**/src-tauri/**", "**/target/**", "**/.git/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: ["es2021", "chrome105", "safari15"],
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  resolve: {
    alias: {
      $lib: "/src/lib",
      $features: "/src/features",
      $components: "/src/components",
    },
  },
});
