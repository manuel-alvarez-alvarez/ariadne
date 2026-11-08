// @vitest-environment jsdom

import { readFileSync } from "node:fs"
import { join } from "node:path"
import { describe, expect, it } from "vitest"
import { markPlatform } from "./platform"

const css = readFileSync(join(process.cwd(), "src/index.css"), "utf8")

const LINUX = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15 (KHTML, like Gecko)"
const MAC = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15"
const ANDROID = "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36"

describe("the Linux scrollbar rule", () => {
  it("marks <html> on Linux and leaves macOS and Android unmarked", () => {
    for (const [ua, marked] of [
      [LINUX, true],
      [MAC, false],
      [ANDROID, false],
    ] as const) {
      const root = document.createElement("html")
      markPlatform(root, ua)
      expect(root.dataset.platform).toBe(marked ? "linux" : undefined)
    }
  })

  it("styles scrollbars only under the Linux marker, with both the standard and the WebKit forms", () => {
    const rules = css.slice(css.indexOf('html[data-platform="linux"]'))
    expect(rules).toContain("scrollbar-width: thin")
    expect(rules).toContain("scrollbar-color")
    expect(rules).toContain('html[data-platform="linux"] ::-webkit-scrollbar-thumb')
    // No unscoped scrollbar rule: it would take macOS's overlay bars away.
    const selectors = css
      .split("\n")
      .filter((line) => /::-webkit-scrollbar/.test(line) && /[{,]$/.test(line))
    expect(selectors.length).toBeGreaterThan(0)
    for (const line of selectors) expect(line).toMatch(/^html\[data-platform="linux"\] /)
  })
})
