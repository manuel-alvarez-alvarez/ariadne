import { fileURLToPath, URL } from "node:url"

import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

// The dev server is what `tauri dev` points its webview at, so the port is
// fixed and failing to get it must be an error rather than a silent fallback.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Rust sources are rebuilt by `tauri dev`, not by Vite.
      ignored: ["**/src-tauri/**"],
    },
  },
  // `tauri dev` sets TAURI_* env vars; expose them alongside VITE_*.
  envPrefix: ["VITE_", "TAURI_"],
  test: {
    // The daemon stub and the jsdom shims have to be in place before a test
    // module — and so `@/api` — is imported. Which environment a file runs in
    // is still the file's own `@vitest-environment` docblock: most of them are
    // pure and have no business paying for a DOM.
    setupFiles: ["./src/test/setup.ts"],
  },
})
