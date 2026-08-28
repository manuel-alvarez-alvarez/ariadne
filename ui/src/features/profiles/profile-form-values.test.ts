/**
 * Tests for the two things in this feature that are easy to get subtly wrong.
 *
 * The first is the daemon's clearing sentinel. `UpdateProfileRequest` puts a
 * profile back on auto with the string `"default"`, while
 * `CreateProfileRequest` takes a plain null for the same state. Getting that
 * backwards writes a profile pinned to a model literally named "default", which
 * nothing else in the stack would catch.
 *
 * The second is the prompt diff. A briefing left at its default must not be
 * sent — on create because the daemon seeds it, on update because a write it
 * did not need is a write that can fail.
 */

import { describe, expect, it } from "vitest"

import type { ProfileDto } from "@/api"
import { aProfile } from "@/test/fixtures"
import {
  changedPrompts,
  emptyProfileFormValues,
  type PromptFormValue,
  profileToFormValues,
  toCreateRequest,
  toUpdateRequest,
} from "./profile-form-values"

/** What an engineer profile is briefed with, as its prompts endpoint answers. */
const BRIEFINGS: PromptFormValue[] = [
  { kind: "engineer_briefing", content: "Start the task." },
  { kind: "changes_requested", content: "Apply the review." },
]

const PROFILE: ProfileDto = aProfile({
  id: "p1",
  name: "rust-engineer",
  model: "claude_code:claude-opus-5",
  system_prompt: "You are a Rust engineer.",
  created_at: "2026-08-16T09:00:00.000Z",
  updated_at: "2026-08-16T09:30:00.000Z",
})

describe("profileToFormValues", () => {
  it("maps a fully specified profile back onto the form", () => {
    expect(profileToFormValues(PROFILE)).toEqual({
      name: "rust-engineer",
      role: "engineer",
      model: "claude_code:claude-opus-5",
      systemPrompt: "You are a Rust engineer.",
      prompts: [],
    })
  })

  it("takes the briefings from the second argument, which the DTO has no room for", () => {
    const values = profileToFormValues(PROFILE, BRIEFINGS)
    expect(values.prompts).toEqual(BRIEFINGS)
  })

  it("shows an unpinned model as the empty box that stands for auto", () => {
    const values = profileToFormValues({ ...PROFILE, model: null })
    expect(values.model).toBe("")
  })
})

describe("toCreateRequest", () => {
  it("sends null — not a sentinel — for auto-resolve", () => {
    expect(toCreateRequest(emptyProfileFormValues("planner"))).toEqual({
      name: "",
      role: "planner",
      model: null,
      // A blank box is no system prompt at all, which is the role's own.
      system_prompt: null,
    })
  })

  it("sends a blank system prompt as none, so the new profile follows its role", () => {
    const values = { ...profileToFormValues(PROFILE), systemPrompt: "   " }
    expect(toCreateRequest(values).system_prompt).toBeNull()
  })

  it("carries no briefings: they are written to the profile once it exists", () => {
    const values = profileToFormValues(PROFILE, BRIEFINGS)
    expect(toCreateRequest(values)).not.toHaveProperty("prompts")
  })

  it("passes a pinned model — agent CLI and all — through", () => {
    const request = toCreateRequest(profileToFormValues(PROFILE))
    expect(request.model).toBe("claude_code:claude-opus-5")
    expect(request.role).toBe("engineer")
  })
})

describe("toUpdateRequest", () => {
  it("clears the model with the daemon's sentinel, which is auto", () => {
    const values = profileToFormValues({ ...PROFILE, model: null })
    expect(toUpdateRequest(values).model).toBe("default")
  })

  it("never sends a role, which the daemon cannot change", () => {
    expect(toUpdateRequest(profileToFormValues(PROFILE))).not.toHaveProperty("role")
  })

  it("keeps a pinned model", () => {
    expect(toUpdateRequest(profileToFormValues(PROFILE)).model).toBe("claude_code:claude-opus-5")
  })
})

describe("changedPrompts", () => {
  it("is empty while nothing has been touched", () => {
    expect(changedPrompts(BRIEFINGS, BRIEFINGS)).toEqual([])
  })

  it("keeps an emptied briefing, which is an edit like any other", () => {
    const edited = [{ kind: "changes_requested" as const, content: "" }, ...BRIEFINGS.slice(2)]
    expect(changedPrompts(edited, BRIEFINGS)).toEqual([{ kind: "changes_requested", content: "" }])
  })

  it("counts a kind the baseline never had as changed", () => {
    expect(changedPrompts(BRIEFINGS, [])).toEqual(BRIEFINGS)
  })
})

describe("whitespace around a name", () => {
  it("is trimmed on both requests", () => {
    const values = { ...profileToFormValues(PROFILE), name: "  spaced  ", model: "  codex  " }
    expect(toCreateRequest(values).name).toBe("spaced")
    expect(toUpdateRequest(values).name).toBe("spaced")
    expect(toCreateRequest(values).model).toBe("codex")
    expect(toUpdateRequest(values).model).toBe("codex")
  })
})
