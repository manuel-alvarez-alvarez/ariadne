#!/usr/bin/env node
// Regenerate the typed API surface from the daemon's OpenAPI document.
//
//   npm run gen:api                     # live daemon on http://127.0.0.1:7676
//   npm run gen:api -- http://host:port # live daemon elsewhere
//   npm run gen:api -- ./some-spec.json # a spec dump on disk
//
// Both artefacts are committed: `openapi.json` (the snapshot the types were
// built from) and `src/api/schema.d.ts` (the generated types). Regenerate and
// commit both whenever the daemon's API changes.

import { readFile, writeFile } from "node:fs/promises"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"

import openapiTS, { astToString } from "openapi-typescript"

const UI_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..")
const SNAPSHOT = resolve(UI_ROOT, "openapi.json")
const OUTPUT = resolve(UI_ROOT, "src/api/schema.d.ts")
const DEFAULT_DAEMON = "http://127.0.0.1:7676"
const SPEC_PATH = "/api-docs/openapi.json"

const BANNER = `/**
 * Types generated from the ariadned OpenAPI document — DO NOT EDIT BY HAND.
 * Regenerate with \`npm run gen:api\` (see ui/README.md).
 */

`

/**
 * utoipa derives `operationId` from the handler function name, so ids collide
 * across tags (`goals::list` and `tasks::list` are both `list`) and OpenAPI
 * requires them to be unique. Qualify each one with its tag before generating;
 * the committed `openapi.json` stays the daemon's verbatim document.
 *
 * Two handlers can also share both a tag and a name — `sessions::input` (the
 * pane) and `console::input` (an acp session's console) are both tagged
 * `sessions` — so a tag alone does not always disambiguate. Anything still
 * colliding after that first pass is qualified again, by its path instead:
 * unlike the function name, no two routes answer to the same one.
 */
function qualifyOperationIds(spec) {
  const operations = []
  for (const [path, pathItem] of Object.entries(spec.paths ?? {})) {
    for (const [method, operation] of Object.entries(pathItem)) {
      if (!operation || typeof operation !== "object" || !operation.operationId) continue
      const tag = operation.tags?.[0]
      if (tag) operation.operationId = `${tag}_${operation.operationId}`
      operations.push({ operation, path, method })
    }
  }
  const counts = new Map()
  for (const { operation } of operations) {
    counts.set(operation.operationId, (counts.get(operation.operationId) ?? 0) + 1)
  }
  for (const { operation, path, method } of operations) {
    if (counts.get(operation.operationId) === 1) continue
    const fromPath = path
      .split("/")
      .filter((segment) => segment && segment !== "v1" && !segment.startsWith("{"))
      .join("_")
    operation.operationId = `${fromPath}_${method}`
  }
  return spec
}

/** Resolve the source argument into the raw OpenAPI document. */
async function loadSpec(source) {
  if (!source) return fetchSpec(DEFAULT_DAEMON + SPEC_PATH)
  if (/^https?:\/\//.test(source)) {
    // Accept both a bare daemon URL and a full spec URL.
    const url = source.endsWith(".json") ? source : source.replace(/\/$/, "") + SPEC_PATH
    return fetchSpec(url)
  }
  return JSON.parse(await readFile(resolve(process.cwd(), source), "utf8"))
}

async function fetchSpec(url) {
  const res = await fetch(url)
  if (!res.ok) throw new Error(`GET ${url} -> ${res.status} ${res.statusText}`)
  return res.json()
}

const source = process.argv[2]
const spec = await loadSpec(source)

await writeFile(SNAPSHOT, `${JSON.stringify(spec, null, 2)}\n`)
const ast = await openapiTS(qualifyOperationIds(spec))
await writeFile(OUTPUT, BANNER + astToString(ast))

console.log(`wrote ${SNAPSHOT}`)
console.log(`wrote ${OUTPUT}`)
