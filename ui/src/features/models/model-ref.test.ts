/**
 * The client's half of the daemon's rule for a model reference.
 *
 * The point of checking here at all is the typo: `claude-opus-5` is a model
 * name a person knows and not something the daemon can run, so the field says
 * that it wants the agent too before the form is submitted. Which agents there
 * are is the daemon's registry, so only the shape is checked here. Everything
 * past that first colon is the agent's business, not ours — an id with colons
 * of its own survives whole — and both the agent and the model are required.
 */

import { describe, expect, it } from "vitest"

import { modelRefError, parseModelRef } from "./model-ref"

describe("modelRefError", () => {
  it("refuses an empty model", () => {
    expect(modelRefError("")).toBe("Choose a model.")
    expect(modelRefError("   ")).toBe("Choose a model.")
  })

  it("takes any agent and any model, one colon apart", () => {
    expect(modelRefError("claude-agent-acp:claude-opus-5")).toBeNull()
    expect(modelRefError("my-agent:some-model")).toBeNull()
    // Only the first colon is structure, so an id with colons of its own is
    // one model and not a malformed reference.
    expect(modelRefError("opencode-acp:ollama/llama3:8b")).toBeNull()
  })

  it("refuses one half on its own by showing where the other goes", () => {
    const message = modelRefError("claude-opus-5")
    expect(message).toContain("<agent>:claude-opus-5")
    expect(message).toContain("claude-opus-5:<model>")
  })

  it("refuses a leading colon, which names no agent", () => {
    expect(modelRefError(":claude-opus-5")).toContain("<agent>:claude-opus-5")
  })

  it("refuses a trailing colon, which names no model", () => {
    expect(modelRefError("codex-acp:")).toBe("Choose a model.")
  })
})

describe("parseModelRef", () => {
  it("splits at the first colon", () => {
    expect(parseModelRef("opencode-acp:ollama/llama3:8b")).toEqual({
      agent: "opencode-acp",
      model: "ollama/llama3:8b",
    })
  })

  it("is null where the text is not a reference", () => {
    expect(parseModelRef("claude-opus-5")).toBeNull()
  })
})
