/**
 * The client's half of the daemon's rule for a model reference.
 *
 * The point of checking here at all is the typo: `claude-opus-5` is a model
 * name a person knows and not something the daemon can run, so the field says
 * which CLI it belongs to before the form is submitted. Everything past that
 * first colon is the CLI's business, not ours — an id with colons of its own
 * survives whole — and an empty box is never an error, since that is how every
 * form says "leave it on the profile's".
 */

import { describe, expect, it } from "vitest"

import { formatModelRef, modelRefError, modelRefLabel } from "./model-ref"

describe("formatModelRef", () => {
  it("spells the two halves a session keeps apart as one id", () => {
    expect(formatModelRef("claude_code", "claude-opus-5")).toBe("claude_code:claude-opus-5")
  })

  it("is the agent CLI alone where the session names no model", () => {
    expect(formatModelRef("codex", null)).toBe("codex")
    expect(formatModelRef("codex")).toBe("codex")
  })
})

describe("modelRefError", () => {
  it("passes an empty box, which is the choice of saying nothing", () => {
    expect(modelRefError("")).toBeNull()
    expect(modelRefError("   ")).toBeNull()
  })

  it("passes an agent CLI, with a model or without one", () => {
    expect(modelRefError("codex")).toBeNull()
    expect(modelRefError("claude_code:claude-opus-5")).toBeNull()
    // Only the first colon is structure, so an id with colons of its own is
    // one model and not a malformed reference.
    expect(modelRefError("opencode:ollama/llama3:8b")).toBeNull()
  })

  it("takes the hyphenated spelling of a CLI, the way the daemon does", () => {
    expect(modelRefError("claude-code:claude-opus-5")).toBeNull()
    expect(modelRefError("claude-code")).toBeNull()
  })

  it("refuses a bare model by naming the CLI it should carry", () => {
    expect(modelRefError("claude-opus-5")).toContain("claude_code:claude-opus-5")
  })

  it("refuses an unknown agent half with the three there are", () => {
    const message = modelRefError("gpt5:latest")
    expect(message).toContain('"gpt5"')
    expect(message).toContain("claude_code, codex, opencode")
  })

  it("refuses a trailing colon, which names a CLI and then no model", () => {
    expect(modelRefError("codex:")).toContain('"codex"')
  })
})

describe("modelRefLabel", () => {
  it("shows a pinned id as itself", () => {
    expect(modelRefLabel("codex:gpt-5.3-codex")).toBe("codex:gpt-5.3-codex")
  })

  it("says `auto` where nothing is pinned, which is a fact and not a blank", () => {
    expect(modelRefLabel(null)).toBe("auto")
    expect(modelRefLabel(undefined)).toBe("auto")
    expect(modelRefLabel("")).toBe("auto")
  })
})
