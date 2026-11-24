import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

// `base` matches where the host app's `main` dev server proxies this
// package's dev server to, and where the packaged app serves its build
// (`ui/README.md`): the iframe `routes/dashboards.tsx` opens is same-origin in
// both.
export default defineConfig({
  base: "/dashboard/",
  plugins: [react()],
  resolve: {
    alias: [
      // The 0.55 Perses packages are `"type": "module"`, so Vite reads their
      // default import of a CommonJS icon file Node-style: the whole
      // `module.exports` object, not the icon component. The package's own
      // ESM copy has a real default export.
      { find: /^mdi-material-ui\/(?!esm\/)(.+)$/, replacement: "mdi-material-ui/esm/$1" },
    ],
  },
  clearScreen: false,
  server: {
    port: 1421,
    strictPort: true,
  },
  build: {
    outDir: "../dist/dashboard",
    emptyOutDir: true,
  },
  test: {
    setupFiles: ["./src/test/setup.ts"],
    reporters: process.env.CI ? ["default", "junit"] : ["default"],
    outputFile: process.env.CI ? { junit: "./junit.xml" } : undefined,
    maxWorkers: 4,
    // The Perses packages are ESM that import CommonJS and extensionless
    // paths (`lodash`, `mdi-material-ui/Circle`) that Node's own loader
    // rejects; Vite's resolver reads them.
    server: { deps: { inline: [/@perses-dev\//] } },
  },
})
