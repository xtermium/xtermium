import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// The UI lives in ui/ and builds to dist/, which tauri.conf.json bundles into the app.
export default defineConfig({
  root: "ui",
  plugins: [svelte()],
  clearScreen: false,
  // No PostCSS; an inline config stops Vite searching parent folders for one.
  css: { postcss: {} },
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/target/**", "**/src/**", "**/rdp/**"] } },
  build: { outDir: "../dist", emptyOutDir: true, target: "es2022" },
});
