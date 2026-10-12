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

const { astToString } = await import(openapiTSPath)

// Generates types for a spec whose one schema is an external `$ref` into a
// YAML file holding `yamlLines`, in a fresh scratch directory each call.
async function generate(yamlLines) {
  const scratch = mkdtempSync(join(tmpdir(), "supply-006-yaml-ref-"))
  try {
    writeFileSync(join(scratch, "external.yaml"), yamlLines.join("\n") + "\n")
    // The same shape gen-api.mjs builds: a plain JS object, as if parsed by
    // `JSON.parse`/`res.json()` — gen-api.mjs never hands openapiTS raw text.
    const spec = {
      openapi: "3.0.3",
      info: { title: "supply-006 proof", version: "0.0.0" },
      paths: {},
      components: { schemas: { Proof: { $ref: "./external.yaml#/schema" } } },
    }
    // A relative $ref resolves against the directory of `cwd`, which
    // openapiTS treats as the root document's own file path.
    const ast = await openapiTS(spec, { cwd: pathToFileURL(join(scratch, "doc.json")) })
    return astToString(ast)
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

// Both fields live only under the anchor. `schema` gets them solely through
// `<<: *base` — the merge-key feature GHSA-2883-xcg3-v3hh concerns and that
// @redocly/openapi-core's parseYaml enables (`implicit: [types.merge]`).
// A two-key merge, not a CPU-exhaustion payload.
const merged = await generate([
  "base: &base",
  "  type: string",
  "  description: from_yaml_merge",
  "schema:",
  "  <<: *base",
])
// Control: the same file with the merge line removed. Without merge-key
// processing, `schema` is empty and inherits nothing from `base`.
const control = await generate([
  "base: &base",
  "  type: string",
  "  description: from_yaml_merge",
  "schema: {}",
])

console.log("--- generated output, with `<<: *base` ---")
console.log(merged)
console.log("--- generated output, control without the merge ---")
console.log(control)

const inherited = (out) => out.includes("from_yaml_merge") && /\bschema: string;/.test(out)
if (!inherited(merged)) {
  console.error("FAIL: with the merge, `schema` did not inherit both `description` and `type: string` from the anchor")
  process.exit(1)
}
if (inherited(control) || control.includes("from_yaml_merge")) {
  console.error("FAIL: the control without a merge still shows the anchored fields — the assertion would not distinguish a merge")
  process.exit(1)
}
console.log("OK: with `<<: *base`, `schema` is `string` described `from_yaml_merge` — both inherited solely from the anchor.")
console.log("OK: the control without the merge carries neither field.")
console.log("This confirms @redocly/openapi-core's YAML merge-key processing ran on externally-referenced content — not that GHSA-2883-xcg3-v3hh's CPU-exhaustion behavior was reproduced.")
