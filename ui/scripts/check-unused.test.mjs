import { spawnSync } from "node:child_process"
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { afterEach, describe, expect, it } from "vitest"

const checker = fileURLToPath(new URL("./check-unused.mjs", import.meta.url))
const roots = []

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { force: true, recursive: true })
})

function fixture(files) {
  const root = mkdtempSync(join(tmpdir(), "check-unused-"))
  roots.push(root)
  write(
    root,
    "package.json",
    JSON.stringify({ name: "fixture", dependencies: { "used-package": "1" } }),
  )
  write(root, "src/main.tsx", 'import "used-package"\n')
  for (const [path, text] of Object.entries(files)) write(root, path, text)
  return root
}

function write(root, path, text) {
  mkdirSync(join(root, path, ".."), { recursive: true })
  writeFileSync(join(root, path), text)
}

function check(root) {
  return spawnSync(process.execPath, [checker], {
    cwd: root,
    env: { ...process.env, CHECK_UNUSED_ROOT: root },
    encoding: "utf8",
  })
}

describe("check:unused", () => {
  it("rejects an unused package", () => {
    const result = check(
      fixture({
        "package.json": JSON.stringify({
          name: "fixture",
          dependencies: { "unused-package": "1", "used-package": "1" },
        }),
      }),
    )

    expect(result.status).not.toBe(0)
    expect(result.stdout).toContain("unused dependencies (1):\n  unused-package")
  })

  it("rejects an unused export", () => {
    const result = check(fixture({ "src/unused-export.ts": "export const unusedExport = true\n" }))

    expect(result.status).not.toBe(0)
    expect(result.stdout).toContain("src/unused-export.ts: unusedExport")
  })

  it("rejects an export that only a test imports", () => {
    const result = check(
      fixture({
        "src/test-only.ts": "export const testOnly = true\n",
        "src/test-only.test.ts": 'import { testOnly } from "./test-only"\nvoid testOnly\n',
      }),
    )

    expect(result.status).not.toBe(0)
    expect(result.stdout).toContain("src/test-only.ts: testOnly")
  })

  it("rejects an orphan source file", () => {
    const result = check(fixture({ "src/orphan.ts": "const unused = true\n" }))

    expect(result.status).not.toBe(0)
    expect(result.stdout).toContain("src/orphan.ts")
  })

  it("keeps test helpers shared by tests", () => {
    const result = check(
      fixture({
        "src/test/helper.ts": "export const helper = true\n",
        "src/first.test.ts": 'import { helper } from "./test/helper"\nvoid helper\n',
        "src/second.test.ts": 'import { helper } from "./test/helper"\nvoid helper\n',
      }),
    )

    expect(result.status).toBe(0)
  })
})
