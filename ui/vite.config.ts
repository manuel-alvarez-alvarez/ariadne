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
    // The Stats screen's iframe opens `/dashboard/`: in a packaged build that
    // is the `ui/dashboard` package's own build, under this one's `dist/`
    // (see its own `vite.config.ts`); in development it is this package's
    // dev server instead, so the iframe stays same-origin either way.
    proxy: {
      "/dashboard": {
        target: "http://localhost:1421",
        changeOrigin: true,
        ws: true,
      },
    },
  },
  // `tauri dev` sets TAURI_* env vars; expose them alongside VITE_*.
  envPrefix: ["VITE_", "TAURI_"],
  test: {
    // `ui/dashboard` is its own package, with its own vitest config and its
    // own `npm test`; left to its default discovery, this config would also
    // pick up its test files and run them against this package's own
    // dependency tree (React 19, `@tanstack/react-query` v5) instead of that
    // package's own (React 18, v4) — silently, since nothing here excludes
    // a sibling directory by default the way `node_modules` is.
    include: ["src/**/*.test.{ts,tsx}", "scripts/**/*.test.mjs"],
    // The daemon stub and the jsdom shims have to be in place before a test
    // module — and so `@/api` — is imported. Which environment a file runs in
    // is still the file's own `@vitest-environment` docblock: most of them are
    // pure and have no business paying for a DOM.
    setupFiles: ["./src/test/setup.ts"],
    // CI turns a failing test into a named annotation (see ci.yml); adding
    // the junit reporter only there keeps a local `npm test` writing nothing
    // extra to disk.
    reporters: process.env.CI ? ["default", "junit"] : ["default"],
    outputFile: process.env.CI ? { junit: "./junit.xml" } : undefined,
    // Capped rather than left at the host's own core count: a worker per
    // core is what turns one slow neighbour process on a shared machine
    // into dozens of starved workers each missing their own test's timeout,
    // not just the one test actually doing the colliding work.
    maxWorkers: 4,
  },
})
