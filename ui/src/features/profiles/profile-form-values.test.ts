/**
 * Tests for the two things in this feature that are easy to get subtly wrong.
 *
 * The first is the daemon's clearing sentinels. `UpdateProfileRequest` clears
 * the agent kind with the string `"auto"` and the model with `"default"`, while
 * `CreateProfileRequest` takes plain nulls for the same two states. Getting that
 * backwards writes a profile pinned to a model literally named "default", which
 * nothing else in the stack would catch.
 *
 * The second is the prompt diff. A briefing left at its default must not be
 * sent — on create because the daemon seeds it, on update because a write it
 * did not need is a write that can fail.
 */

import { describe, expect, it } from "vitest"

import type { ProfileDto } from "@/api"

import {
  changedPrompts,
  emptyProfileFormValues,
  type PromptFormValue,
  profileToFormValues,
  toCreateRequest,
  toUpdateRequest,
} from "./profile-form-values"

/** What an engineer role seeds, as the defaults endpoint answers it. */
const DEFAULTS: PromptFormValue[] = [
  { kind: "engineer_briefing", content: "Start the task." },
  { kind: "changes_requested", content: "Apply the review." },
]

const PROFILE: ProfileDto = {
  id: "p1",
  name: "rust-engineer",
  role: "engineer",
  agent_kind: "claude_code",
  model: "claude-opus-5",
  system_prompt: "You are a Rust engineer.",
  created_at: "2026-08-16T09:00:00.000Z",
  updated_at: "2026-08-16T09:30:00.000Z",
}

describe("profileToFormValues", () => {
  it("maps a fully specified profile back onto the form", () => {
    expect(profileToFormValues(PROFILE)).toEqual({
      name: "rust-engineer",
      role: "engineer",
      agentKind: "claude_code",
      model: "claude-opus-5",
      systemPrompt: "You are a Rust engineer.",
      prompts: [],
    })
  })

  it("takes the briefings from the second argument, which the DTO has no room for", () => {
    const values = profileToFormValues(PROFILE, DEFAULTS)
    expect(values.prompts).toEqual(DEFAULTS)
  })

  it("shows the unset agent kind and model as their explicit choices", () => {
    const values = profileToFormValues({ ...PROFILE, agent_kind: null, model: null })
    expect(values.agentKind).toBe("auto")
    expect(values.model).toBe("")
  })
})

describe("toCreateRequest", () => {
  it("sends null — not a sentinel — for auto-resolve and the provider default", () => {
    expect(toCreateRequest(emptyProfileFormValues("planner"))).toEqual({
      name: "",
      role: "planner",
      agent_kind: null,
      model: null,
      system_prompt: "",
    })
  })

  it("leaves the prompts out when every briefing is still its default", () => {
    const values = { ...profileToFormValues(PROFILE, DEFAULTS) }
    expect(toCreateRequest(values, DEFAULTS)).not.toHaveProperty("prompts")
  })

  it("sends only the briefings edited away from their default", () => {
    const values = profileToFormValues(PROFILE, [
      { kind: "engineer_briefing", content: "Start the task, carefully." },
      ...DEFAULTS.slice(1),
    ])
    expect(toCreateRequest(values, DEFAULTS).prompts).toEqual([
      { kind: "engineer_briefing", content: "Start the task, carefully." },
    ])
  })

  it("sends nothing when the defaults never arrived, so the daemon seeds them all", () => {
    const values = { ...profileToFormValues(PROFILE), prompts: [] }
    expect(toCreateRequest(values, []).prompts).toBeUndefined()
  })

  it("passes a pinned agent kind and model through", () => {
    const request = toCreateRequest(profileToFormValues(PROFILE))
    expect(request.agent_kind).toBe("claude_code")
    expect(request.model).toBe("claude-opus-5")
    expect(request.role).toBe("engineer")
  })
})

describe("toUpdateRequest", () => {
  it("clears the agent kind and model with the daemon's sentinels", () => {
    const values = profileToFormValues({ ...PROFILE, agent_kind: null, model: null })
    const request = toUpdateRequest(values)
    expect(request.agent_kind).toBe("auto")
    expect(request.model).toBe("default")
  })

  it("never sends a role, which the daemon cannot change", () => {
    expect(toUpdateRequest(profileToFormValues(PROFILE))).not.toHaveProperty("role")
  })

  it("keeps a pinned agent kind and model", () => {
    const request = toUpdateRequest(profileToFormValues(PROFILE))
    expect(request.agent_kind).toBe("claude_code")
    expect(request.model).toBe("claude-opus-5")
  })
})

describe("changedPrompts", () => {
  it("is empty while nothing has been touched", () => {
    expect(changedPrompts(DEFAULTS, DEFAULTS)).toEqual([])
  })

  it("keeps an emptied briefing, which is an edit like any other", () => {
    const edited = [{ kind: "changes_requested" as const, content: "" }, ...DEFAULTS.slice(2)]
    expect(changedPrompts(edited, DEFAULTS)).toEqual([{ kind: "changes_requested", content: "" }])
  })

  it("counts a kind the baseline never had as changed", () => {
    expect(changedPrompts(DEFAULTS, [])).toEqual(DEFAULTS)
  })
})

describe("whitespace around a name", () => {
  it("is trimmed on both requests", () => {
    const values = { ...profileToFormValues(PROFILE), name: "  spaced  ", model: "  gpt-5  " }
    expect(toCreateRequest(values).name).toBe("spaced")
    expect(toUpdateRequest(values).name).toBe("spaced")
    expect(toCreateRequest(values).model).toBe("gpt-5")
    expect(toUpdateRequest(values).model).toBe("gpt-5")
  })
})
