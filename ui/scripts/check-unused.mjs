/**
 * Two audits the repository's shape depends on, run over `src/`:
 *
 * - every `dependencies` / `devDependencies` entry is reached from somewhere in
 *   the app (a TS/TSX import, a CSS `@import`, a config file);
 * - every exported symbol is *imported* by a non-test file other than the one
 *   that declares it;
 * - every source file is imported by another file, unless it is an entry point
 *   or a test.
 *
 * Both print what they found and exit non-zero when anything is unused, so a
 * dependency or an export that stops being reachable is a failing check rather
 * than something an audit has to notice again later.
 */

import { readdirSync, readFileSync } from "node:fs"
import { dirname, join, normalize } from "node:path"
import { fileURLToPath } from "node:url"

const root = process.env.CHECK_UNUSED_ROOT ?? fileURLToPath(new URL("..", import.meta.url))

/** Files whose exports are a public surface with no in-repo caller. */
const ENTRYPOINTS = new Set(["src/main.tsx"])
/** Generated; not ours to prune. */
const GENERATED = new Set(["src/api/schema.d.ts"])
const SOURCE_EXTENSIONS = [".ts", ".tsx", ".css"]

function isTestFile(path) {
  return /(?:\.test\.(?:ts|tsx)|(?:^|\/)(?:setup|[^/]+\.setup)\.(?:ts|tsx)|^src\/test\/)/.test(path)
}

function walk(dir, out = []) {
  for (const entry of readdirSync(join(root, dir), { withFileTypes: true })) {
    const path = `${dir}/${entry.name}`
    if (entry.isDirectory()) walk(path, out)
    else if (SOURCE_EXTENSIONS.some((extension) => path.endsWith(extension))) out.push(path)
  }
  return out
}

const files = walk("src")
const sources = new Map(files.map((path) => [path, readFileSync(join(root, path), "utf8")]))
for (const path of ["vite.config.ts", "index.html", "components.json"]) {
  try {
    sources.set(path, readFileSync(join(root, path), "utf8"))
  } catch {
    // Fixtures only need the source tree.
  }
}
try {
  for (const path of readdirSync(join(root, "scripts"))) {
    sources.set(`scripts/${path}`, readFileSync(join(root, "scripts", path), "utf8"))
  }
} catch {
  // Fixtures only need the source tree.
}

let failed = false

// ── dependencies ──────────────────────────────────────────────────────────
const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"))
/** Packages nothing imports by name because a tool loads them by config. */
const TOOLING = new Set([
  "@biomejs/biome",
  "@tauri-apps/cli",
  "@types/node",
  "@types/react",
  "@types/react-dom",
  "jsdom",
  "openapi-typescript",
  "tailwindcss",
  "typescript",
  "vite",
  "vitest",
])
const declared = [
  ...Object.keys(pkg.dependencies ?? {}),
  ...Object.keys(pkg.devDependencies ?? {}),
].filter((name) => !TOOLING.has(name))

const unusedDeps = declared.filter((name) => {
  // The package as it is written in a specifier: `"pkg"` or `"pkg/entry"`,
  // which covers `import`, `@import` and `require` alike.
  const specifier = new RegExp(`["']${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}(/|["'])`)
  for (const text of sources.values()) if (specifier.test(text)) return false
  return true
})
if (unusedDeps.length > 0) {
  failed = true
  console.log(`unused dependencies (${unusedDeps.length}):`)
  for (const name of unusedDeps) console.log(`  ${name}`)
} else {
  console.log(`dependencies: all ${declared.length} declared packages are imported in ui/`)
}

// ── exports ───────────────────────────────────────────────────────────────

/**
 * What a module *imports by name*, which is the only thing that makes another
 * module's export used.
 *
 * Matching bare identifiers instead would report a false clean: `TaskUpdatedDto`
 * appears in `schema.d.ts` as a component name and `MIN_FONT_SIZE` appears in a
 * comment two files away, and neither is a reference.
 */
const IMPORT_SPECIFIERS = /(?:^|\n)\s*(?:import|export)\s+(?:type\s+)?\{([^}]*)\}\s*from\s*["']/g
/** `import Thing from "…"`, and the namespace form. */
const DEFAULT_IMPORT =
  /(?:^|\n)\s*import\s+(?:type\s+)?(?:\*\s+as\s+)?([A-Za-z_$][\w$]*)\s*(?:,|from)/g

/** Comments are prose, and prose naming an export is not a use of it. */
function stripComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, "").replace(/(^|[^:])\/\/.*$/gm, "$1")
}

function importedNames(text) {
  const names = new Set()
  const code = stripComments(text)
  for (const [, group] of code.matchAll(IMPORT_SPECIFIERS)) {
    for (const part of group.split(",")) {
      const name = part
        .trim()
        .replace(/^type\s+/, "")
        .split(/\s+as\s+/)[0]
        ?.trim()
      if (name && /^[A-Za-z_$][\w$]*$/.test(name)) names.add(name)
    }
  }
  for (const [, name] of code.matchAll(DEFAULT_IMPORT)) names.add(name)
  return names
}

const DECLARATION =
  /^export\s+(?:async\s+)?(?:default\s+)?(?:abstract\s+)?(?:const|let|var|function|class|type|interface|enum)\s+([A-Za-z_$][\w$]*)/gm
const NAMED = /^export\s*\{([^}]*)\}/gms

function exportedNames(text) {
  const names = new Set()
  for (const [, name] of text.matchAll(DECLARATION)) names.add(name)
  for (const [, group] of text.matchAll(NAMED)) {
    for (const part of group.split(",")) {
      const name = part
        .trim()
        .replace(/^type\s+/, "")
        .split(/\s+as\s+/)
        .pop()
        ?.trim()
      if (name && /^[A-Za-z_$][\w$]*$/.test(name)) names.add(name)
    }
  }
  return names
}

/**
 * Every name imported anywhere in the app, by the file that imports it.
 *
 * The generated schema is skipped: it declares nothing the app imports by name,
 * and reading it would let its component names stand in for real references.
 */
const importedFrom = new Map()
for (const [path, text] of sources) {
  if (GENERATED.has(path) || path.endsWith(".css") || isTestFile(path)) continue
  importedFrom.set(path, importedNames(text))
}

const unusedExports = []
for (const [path, text] of sources) {
  if (!path.startsWith("src/") || GENERATED.has(path) || ENTRYPOINTS.has(path) || isTestFile(path))
    continue
  if (path.endsWith(".css")) continue
  for (const name of exportedNames(text)) {
    let seen = false
    for (const [other, names] of importedFrom) {
      if (other === path) continue
      if (names.has(name)) {
        seen = true
        break
      }
    }
    if (!seen) unusedExports.push(`${path}: ${name}`)
  }
}
if (unusedExports.length > 0) {
  failed = true
  console.log(`\nexports referenced nowhere outside their own file (${unusedExports.length}):`)
  for (const line of unusedExports) console.log(`  ${line}`)
} else {
  console.log("exports: every production export in src/ is referenced by another production file")
}

// ── source files ─────────────────────────────────────────────────────────

const MODULE_SPECIFIER = /(?:^|\n)\s*(?:import|export)(?:\s+[\s\S]*?\s+from)?\s*["']([^"']+)["']/g
const DYNAMIC_IMPORT = /\bimport\s*\(\s*["']([^"']+)["']\s*\)/g

function moduleSpecifiers(text) {
  const specifiers = new Set()
  const code = stripComments(text)
  for (const [, specifier] of code.matchAll(MODULE_SPECIFIER)) specifiers.add(specifier)
  for (const [, specifier] of code.matchAll(DYNAMIC_IMPORT)) specifiers.add(specifier)
  return specifiers
}

function importedFile(path, specifier) {
  const clean = specifier.split(/[?#]/, 1)[0]
  let base
  if (clean.startsWith("@/")) base = `src/${clean.slice(2)}`
  else if (clean.startsWith(".")) base = normalize(join(dirname(path), clean))
  else return undefined

  const candidates = [
    base,
    ...SOURCE_EXTENSIONS.map((extension) => `${base}${extension}`),
    ...SOURCE_EXTENSIONS.map((extension) => `${base}/index${extension}`),
  ]
  return candidates.find((candidate) => sources.has(candidate))
}

const importedFiles = new Set()
for (const [path, text] of sources) {
  for (const specifier of moduleSpecifiers(text)) {
    const target = importedFile(path, specifier)
    if (target) importedFiles.add(target)
  }
}

const orphanFiles = files.filter(
  (path) =>
    !GENERATED.has(path) && !ENTRYPOINTS.has(path) && !isTestFile(path) && !importedFiles.has(path),
)
if (orphanFiles.length > 0) {
  failed = true
  console.log(`\norphan source files (${orphanFiles.length}):`)
  for (const path of orphanFiles) console.log(`  ${path}`)
} else {
  console.log("source files: every non-test file in src/ is imported")
}

process.exit(failed ? 1 : 0)
