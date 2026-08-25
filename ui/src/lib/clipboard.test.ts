import { describe, expect, it, vi } from "vitest"

import { copyText, goalCopyEntries, sessionCopyEntries, taskCopyEntries } from "./clipboard"

/**
 * The tests run in node, where neither API exists, so both routes are stubbed:
 * what matters is which one is taken, and that a refusal of the first is not
 * the end of it — that is the whole point of the fallback in the Tauri webview.
 */

interface Stubs {
  writeText?: (text: string) => Promise<void>
  execCommand?: () => boolean
}

function stub({ writeText, execCommand }: Stubs) {
  const textarea = { value: "", setAttribute: vi.fn(), style: {}, select: vi.fn(), remove: vi.fn() }
  vi.stubGlobal("navigator", writeText ? { clipboard: { writeText } } : {})
  vi.stubGlobal("document", {
    createElement: () => textarea,
    body: { append: vi.fn() },
    execCommand: execCommand ?? (() => false),
  })
  return textarea
}

describe("copyText", () => {
  it("uses the clipboard API when it is there", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    const textarea = stub({ writeText })

    await expect(copyText("01ARZ3NDEK")).resolves.toBe(true)
    expect(writeText).toHaveBeenCalledWith("01ARZ3NDEK")
    expect(textarea.select).not.toHaveBeenCalled()
  })

  it("falls back to a textarea when the clipboard API is missing", async () => {
    const textarea = stub({ execCommand: () => true })

    await expect(copyText("01ARZ3NDEK")).resolves.toBe(true)
    expect(textarea.value).toBe("01ARZ3NDEK")
    expect(textarea.select).toHaveBeenCalled()
    expect(textarea.remove).toHaveBeenCalled()
  })

  it("falls back to a textarea when the clipboard API rejects", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("NotAllowedError"))
    const textarea = stub({ writeText, execCommand: () => true })

    await expect(copyText("01ARZ3NDEK")).resolves.toBe(true)
    expect(textarea.select).toHaveBeenCalled()
  })

  it("reports failure when neither route works", async () => {
    const textarea = stub({ execCommand: () => false })

    await expect(copyText("01ARZ3NDEK")).resolves.toBe(false)
    expect(textarea.remove).toHaveBeenCalled()
  })
})

/**
 * These strings are pasted into a shell, so they are checked whole: a wrong
 * subcommand fails only in the user's terminal, long after it was copied. The
 * id is the 26-character ULID in full — the menu is reached from a display that
 * may have shortened it.
 */

const ID = "01ARZ3NDEKTSV4RRFFQ69G5FAV"

describe("goalCopyEntries", () => {
  it("offers the id and the command that attaches to it", () => {
    expect(goalCopyEntries(ID)).toEqual([
      { label: "Copy goal ID", text: ID },
      { label: "Copy attach command", text: `ariadne attach ${ID}` },
    ])
  })
})

describe("taskCopyEntries", () => {
  it("offers the id and every command that takes a task id", () => {
    expect(taskCopyEntries(ID)).toEqual([
      { label: "Copy task ID", text: ID },
      { label: "Copy attach command", text: `ariadne attach ${ID}` },
      { label: "Copy logs command", text: `ariadne task logs ${ID}` },
      { label: "Copy diff command", text: `ariadne task diff ${ID}` },
    ])
  })
})

describe("sessionCopyEntries", () => {
  it("offers the id and every command that takes a session id", () => {
    expect(sessionCopyEntries(ID)).toEqual([
      { label: "Copy session ID", text: ID },
      { label: "Copy attach command", text: `ariadne attach ${ID}` },
      { label: "Copy logs command", text: `ariadne session logs ${ID}` },
    ])
  })
})
