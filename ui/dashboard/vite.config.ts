import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

// `base` matches where the host app's `main` dev server proxies this
// package's dev server to, and where the packaged app serves its build
// (`ui/README.md`): the iframe `routes/dashboards.tsx` opens is same-origin in
// both.
export default defineConfig({
  base: "/dashboard/",
  plugins: [react()],
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
