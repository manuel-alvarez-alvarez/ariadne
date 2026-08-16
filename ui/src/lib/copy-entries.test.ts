import { describe, expect, it } from "vitest"

import { goalCopyEntries, sessionCopyEntries, taskCopyEntries } from "./copy-entries"

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
