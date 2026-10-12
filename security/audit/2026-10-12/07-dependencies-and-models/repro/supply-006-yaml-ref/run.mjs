#!/usr/bin/env node
// Bounded, harmless reproduction: proves @redocly/openapi-core's external
// $ref resolution parses YAML with merge-key support, via the exact
// `openapiTS()` call ui/scripts/gen-api.mjs:97 makes. See README.md in this
// directory for what this does and does not prove.
//
// Finds the repository root by walking up from this file until a
// `ui/node_modules` directory exists, so it runs regardless of where the
// repository is checked out, as long as `npm ci` has run in `ui/`.
import { mkdtempSync, writeFileSync, rmSync, existsSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, dirname } from "node:path"
import { fileURLToPath, pathToFileURL } from "node:url"

function findRepoRoot(startDir) {
  let dir = startDir
  for (let i = 0; i < 10; i++) {
    if (existsSync(join(dir, "ui", "node_modules", "openapi-typescript"))) return dir
    const parent = dirname(dir)
    if (parent === dir) break
    dir = parent
  }
  throw new Error("could not find ui/node_modules/openapi-typescript above " + startDir + " — run `npm ci` in ui/ first")
}

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = findRepoRoot(here)
const openapiTSPath = pathToFileURL(join(repoRoot, "ui/node_modules/openapi-typescript/dist/index.mjs")).href
const { default: openapiTS } = await import(openapiTSPath)

const scratch = mkdtempSync(join(tmpdir(), "supply-006-yaml-ref-"))
try {
  // An external YAML file with an anchor merged via `<<:` — the exact
  // feature GHSA-2883-xcg3-v3hh's `maxTotalMergeKeys` advisory is about,
  // and the exact schema feature @redocly/openapi-core's parseYaml enables
  // (`implicit: [js_yaml_1.types.merge]`). This is a two-key merge, not a
  // CPU-exhaustion payload.
  const yamlPath = join(scratch, "external.yaml")
  writeFileSync(
    yamlPath,
    [
      "base: &base",
      "  type: string",
      "schema:",
      "  <<: *base",
      "  description: from_yaml_merge",
      "",
    ].join("\n"),
  )

  // The same shape gen-api.mjs builds: a plain JS object, as if parsed by
  // `JSON.parse`/`res.json()` — gen-api.mjs never hands openapiTS raw text.
  const spec = {
    openapi: "3.0.3",
    info: { title: "supply-006 proof", version: "0.0.0" },
    paths: {},
    components: {
      schemas: {
        Proof: { $ref: `./external.yaml#/schema` },
      },
    },
  }
  // openapiTS resolves a relative $ref against the *directory* of its `cwd`
  // option, treated as the root document's own file path (not a directory
  // path) — so this must name a file inside `scratch`, not `scratch` itself.
  const ast = await openapiTS(spec, { cwd: pathToFileURL(join(scratch, "doc.json")) })
  const { astToString } = await import(
    pathToFileURL(join(repoRoot, "ui/node_modules/openapi-typescript/dist/index.mjs")).href
  )
  const output = astToString(ast)

  console.log("--- generated output ---")
  console.log(output)

  if (!output.includes("from_yaml_merge")) {
    console.error("FAIL: generated output does not contain the merged YAML field — the external $ref/YAML-merge path was not exercised as expected")
    process.exit(1)
  }
  console.log("\nOK: the generated output contains the field produced only by resolving external.yaml's `<<: *base` merge.")
  console.log("This confirms @redocly/openapi-core's YAML merge-key parser ran on externally-referenced content — not that GHSA-2883-xcg3-v3hh's CPU-exhaustion behavior was reproduced.")
} finally {
  rmSync(scratch, { recursive: true, force: true })
}
