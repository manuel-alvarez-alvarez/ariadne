import { describe, expect, it } from "vitest"

import { highlight, knownLanguage, languageForPath, languageLabel, loadGrammar } from "./highlight"
import { HIGHLIGHT_STYLES } from "./highlight-style"

async function tokens(code: string, fence: string, dark = false) {
  const language = knownLanguage(fence)
  if (!language) throw new Error(`unknown fence ${fence}`)
  const grammar = await loadGrammar(language)
  if (!grammar) throw new Error(`no grammar for ${fence}`)
  return highlight(code, grammar, HIGHLIGHT_STYLES[dark ? "dark" : "light"])
}

const classOf = (list: { text: string; className?: string }[], text: string) =>
  list.find((token) => token.text === text)?.className

it("names a language by its fence or its extension, and labels it", () => {
  expect(knownLanguage("ts")).toBe("typescript")
  expect(knownLanguage("py")).toBe("python")
  expect(knownLanguage("golang")).toBe("go")
  expect(knownLanguage("console")).toBe("shell")
  expect(languageLabel(knownLanguage("rs") ?? "")).toBe("Rust")
  expect(languageLabel(knownLanguage("kt") ?? "")).toBe("Kotlin")
})

it("leaves a name the data does not know, or plain text, unknown", () => {
  expect(knownLanguage("mystery")).toBeUndefined()
  expect(knownLanguage("text")).toBeUndefined()
  expect(knownLanguage(undefined)).toBeUndefined()
})

it("reads a file's language from its extension", () => {
  expect(languageForPath("src/app.ts")).toBe("typescript")
  expect(languageForPath("crates/main.rs")).toBe("rust")
  expect(languageForPath("README")).toBeUndefined()
})

it("reads an extension that is not only letters and digits", () => {
  expect(languageForPath("src/main.c++")).toBe("c++")
  expect(languageForPath("src/main.h++")).toBe("c++")
})

it("leaves a fence named after an object property unknown", () => {
  expect(knownLanguage("constructor")).toBeUndefined()
  expect(knownLanguage("__proto__")).toBeUndefined()
  expect(knownLanguage("toString")).toBeUndefined()
})

it("loses no text", async () => {
  const code = `const n = 42 // answer\nlet s = "a // not a comment"\n`
  const list = await tokens(code, "ts")
  expect(list.map((token) => token.text).join("")).toBe(code)
})

it("colours a JSON property name and a string value differently", async () => {
  const list = await tokens(`{"name": "value"}`, "json")
  expect(classOf(list, `"name"`)).toBeTruthy()
  expect(classOf(list, `"value"`)).toBeTruthy()
  expect(classOf(list, `"name"`)).not.toBe(classOf(list, `"value"`))
})

it("colours Markdown headings, emphasis, links and fences", async () => {
  const list = await tokens("# Title\n\n*soft* [link](https://a.b)\n\n```\nx\n```\n", "md")
  expect(classOf(list, " Title")).toBeTruthy()
  expect(classOf(list, "soft")).toBeTruthy()
  expect(classOf(list, "link")).toBeTruthy()
  expect(classOf(list, "```")).toBeTruthy()
})

it("colours HTML tags and attributes", async () => {
  const list = await tokens(`<a href="x">y</a>`, "html")
  expect(classOf(list, "a")).toBeTruthy()
  expect(classOf(list, "href")).toBeTruthy()
  expect(classOf(list, "a")).not.toBe(classOf(list, "href"))
})

it("keeps a Python triple-quoted string as one string token", async () => {
  const text = `"""one\ntwo"""`
  const list = await tokens(`x = ${text}\n`, "python")
  expect(classOf(list, text)).toBeTruthy()
})

it("keeps a shell heredoc as one string token", async () => {
  const list = await tokens("cat <<EOF\nhello\nworld\nEOF\n", "sh")
  expect(list.some((token) => token.className && token.text.includes("hello\nworld"))).toBe(true)
})

describe("the two themes", () => {
  it("colour by the light and the dark style of the task diff viewer", async () => {
    const light = classOf(await tokens("let x = 1", "js"), "let")
    const dark = classOf(await tokens("let x = 1", "js", true), "let")
    expect(light).toBeTruthy()
    expect(dark).toBeTruthy()
    expect(light).not.toBe(dark)
  })
})
