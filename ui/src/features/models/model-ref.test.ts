/**
 * The client's half of the daemon's rule for a model reference.
 *
 * The point of checking here at all is the typo: `claude-opus-5` is a model
 * name a person knows and not something the daemon can run, so the field says
 * which CLI it belongs to before the form is submitted. Everything past that
 * first colon is the CLI's business, not ours — an id with colons of its own
 * survives whole — and both the agent and the model are required.
 */

import { describe, expect, it } from "vitest"

import { formatModelRef, modelRefError } from "./model-ref"

describe("formatModelRef", () => {
  it("spells the two halves a session keeps apart as one id", () => {
    expect(formatModelRef("claude_code", "claude-opus-5")).toBe("claude_code:claude-opus-5")
  })
})

describe("modelRefError", () => {
  it("refuses an empty model", () => {
    expect(modelRefError("")).toBe("Choose a model.")
    expect(modelRefError("   ")).toBe("Choose a model.")
  })

  it("requires a model after the agent CLI", () => {
    expect(modelRefError("codex")).toContain("codex:<model>")
    expect(modelRefError("claude_code:claude-opus-5")).toBeNull()
    // Only the first colon is structure, so an id with colons of its own is
    // one model and not a malformed reference.
    expect(modelRefError("opencode:ollama/llama3:8b")).toBeNull()
  })

  it("takes the hyphenated spelling of a CLI, the way the daemon does", () => {
    expect(modelRefError("claude-code:claude-opus-5")).toBeNull()
    expect(modelRefError("claude-code")).toContain("claude-code:<model>")
  })

  it("refuses a bare model by naming the CLI it should carry", () => {
    expect(modelRefError("claude-opus-5")).toContain("claude_code:claude-opus-5")
  })

  it("refuses an unknown agent half with the three there are", () => {
    const message = modelRefError("gpt5:latest")
    expect(message).toContain('"gpt5"')
    expect(message).toContain("claude_code, codex, opencode")
  })

  it("refuses a trailing colon, which names no model", () => {
    expect(modelRefError("codex:")).toBe("Choose a model.")
  })
})
