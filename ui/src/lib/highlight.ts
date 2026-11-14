/**
 * A small syntax highlighter for the code the console and the Markdown
 * renderer show: comments, strings, numbers and keywords, for the languages
 * agents write most. A language it does not know comes back as one plain
 * token, so unknown code is shown as it is.
 */

export type TokenKind = "comment" | "string" | "number" | "keyword"

type Token = { text: string; kind?: TokenKind }

type Family = {
  label: string
  /** The comment syntax: C-style (line and block), hash, or none for data formats. */
  comments: "c" | "hash" | "none"
  keywords: ReadonlySet<string>
}

const words = (list: string) => new Set(list.split(" "))

const LITERALS = "true false null"
const JS = words(
  `${LITERALS} undefined const let var function return if else for while do switch case break continue new class extends import export from default async await try catch finally throw typeof instanceof in of interface type enum implements public private protected readonly static void this super`,
)
const PYTHON = words(
  "True False None def class return if elif else for while in not and or is import from as with try except finally raise lambda yield pass break continue async await global nonlocal del assert",
)
const RUST = words(
  `${LITERALS} fn let mut const static struct enum impl trait pub use mod crate self Self super match if else for while loop in return break continue move async await where as ref type dyn unsafe`,
)
const GO = words(
  "true false nil func var const type struct interface map chan package import return if else for range switch case default break continue go defer select fallthrough goto",
)
const SHELL = words(
  "if then else elif fi for while until do done case esac in function return export local readonly set unset exit echo cd",
)
const JAVA = words(
  `${LITERALS} class interface enum extends implements public private protected static final abstract void new return if else for while do switch case break continue try catch finally throw throws import package this super int long double float boolean char byte short var`,
)
const C = words(
  `${LITERALS} struct union enum typedef static const extern void int long double float char short unsigned signed return if else for while do switch case break continue sizeof goto class public private protected namespace using template typename new delete nullptr`,
)

const FAMILIES: Record<string, Family> = {
  javascript: { label: "JavaScript", comments: "c", keywords: JS },
  typescript: { label: "TypeScript", comments: "c", keywords: JS },
  python: { label: "Python", comments: "hash", keywords: PYTHON },
  rust: { label: "Rust", comments: "c", keywords: RUST },
  go: { label: "Go", comments: "c", keywords: GO },
  shell: { label: "Shell", comments: "hash", keywords: SHELL },
  java: { label: "Java", comments: "c", keywords: JAVA },
  c: { label: "C/C++", comments: "c", keywords: C },
  json: { label: "JSON", comments: "none", keywords: words(LITERALS) },
  yaml: { label: "YAML", comments: "hash", keywords: words(LITERALS) },
  toml: { label: "TOML", comments: "hash", keywords: words(LITERALS) },
  css: { label: "CSS", comments: "c", keywords: words("") },
  sql: { label: "SQL", comments: "none", keywords: words("") },
}

const ALIASES: Record<string, string> = {
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  javascript: "javascript",
  ts: "typescript",
  tsx: "typescript",
  typescript: "typescript",
  py: "python",
  python: "python",
  rs: "rust",
  rust: "rust",
  go: "go",
  golang: "go",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  shell: "shell",
  console: "shell",
  java: "java",
  kt: "java",
  c: "c",
  h: "c",
  cc: "c",
  cpp: "c",
  hpp: "c",
  json: "json",
  jsonc: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  css: "css",
  sql: "sql",
}

/** The language a fence or a file extension names, or undefined where it is not known. */
export function knownLanguage(name: string | undefined): string | undefined {
  return name === undefined ? undefined : ALIASES[name.toLowerCase()]
}

/** The label shown for a language `knownLanguage` returned. */
export function languageLabel(language: string): string {
  return FAMILIES[language]?.label ?? language
}

/** The language of a file path by its extension. */
export function languageForPath(path: string): string | undefined {
  const extension = /\.([A-Za-z0-9]+)$/.exec(path.trim())?.[1]
  return knownLanguage(extension)
}

const COMMENT = { c: String.raw`\/\/.*|\/\*[\s\S]*?(?:\*\/|$)`, hash: "#.*", none: "(?!)" }
const STRING = [
  String.raw`"(?:[^"\\\n]|\\.)*"`,
  String.raw`'(?:[^'\\\n]|\\.)*'`,
  String.raw`\x60(?:[^\x60\\]|\\.)*\x60?`,
].join("|")
const NUMBER = String.raw`\b0x[\da-fA-F_]+\b|\b\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?\b`
const WORD = String.raw`[A-Za-z_$][\w$]*`

const patterns = new Map<string, RegExp>()

function pattern(family: Family): RegExp {
  const source = `(${COMMENT[family.comments]})|(${STRING})|(${NUMBER})|(${WORD})`
  let found = patterns.get(source)
  if (!found) {
    found = new RegExp(source, "g")
    patterns.set(source, found)
  }
  found.lastIndex = 0
  return found
}

/** Split `code` into tokens; the tokens joined are always `code`. */
export function highlight(code: string, language: string | undefined): Token[] {
  const family = language === undefined ? undefined : FAMILIES[language]
  if (!family) return [{ text: code }]
  const tokens: Token[] = []
  const re = pattern(family)
  let at = 0
  const push = (text: string, kind?: TokenKind) => {
    if (text === "") return
    const last = tokens.at(-1)
    if (last && last.kind === kind) last.text += text
    else tokens.push(kind ? { text, kind } : { text })
  }
  for (let match = re.exec(code); match !== null; match = re.exec(code)) {
    if (match[0] === "") {
      re.lastIndex += 1
      continue
    }
    push(code.slice(at, match.index))
    at = match.index + match[0].length
    if (match[1] !== undefined) push(match[0], "comment")
    else if (match[2] !== undefined) push(match[0], "string")
    else if (match[3] !== undefined) push(match[0], "number")
    else push(match[0], family.keywords.has(match[0]) ? "keyword" : undefined)
  }
  push(code.slice(at))
  return tokens
}
