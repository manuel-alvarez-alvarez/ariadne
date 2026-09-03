/**
 * Tests for the two things in this feature that are easy to get subtly wrong.
 *
 * The first is the daemon's clearing sentinel. `UpdateProfileRequest` puts a
 * profile back on auto with the string `"default"` — for the model and for the
 * effort beside it — while `CreateProfileRequest` takes a plain null for the
 * same state. Getting that backwards writes a profile pinned to a model
 * literally named "default", which nothing else in the stack would catch.
 *
 */

import { describe, expect, it } from "vitest"

import type { ProfileDto } from "@/api"
import { aProfile } from "@/test/fixtures"
import {
  emptyProfileFormValues,
  profileToFormValues,
  toCreateRequest,
  toUpdateRequest,
} from "./profile-form-values"

const PROFILE: ProfileDto = aProfile({
  id: "p1",
  name: "rust-engineer",
  model: "claude_code:claude-opus-5",
  effort: "xhigh",
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
      effort: "xhigh",
      systemPrompt: "You are a Rust engineer.",
    })
  })

  it("shows an unpinned model as the empty box that stands for auto", () => {
    const values = profileToFormValues({ ...PROFILE, model: null })
    expect(values.model).toBe("")
  })

  it("shows an unpinned effort the same way: the empty box is the CLI's own", () => {
    const values = profileToFormValues({ ...PROFILE, effort: null })
    expect(values.effort).toBe("")
  })
})

describe("toCreateRequest", () => {
  it("sends null — not a sentinel — for auto-resolve", () => {
    expect(toCreateRequest(emptyProfileFormValues("planner"))).toEqual({
      name: "",
      role: "planner",
      model: null,
      effort: null,
      // A blank box is no system prompt at all, which is the role's own.
      system_prompt: null,
    })
  })

  it("sends a blank system prompt as none, so the new profile follows its role", () => {
    const values = { ...profileToFormValues(PROFILE), systemPrompt: "   " }
    expect(toCreateRequest(values).system_prompt).toBeNull()
  })

  it("passes a pinned model — agent CLI and all — through, at its effort", () => {
    const request = toCreateRequest(profileToFormValues(PROFILE))
    expect(request.model).toBe("claude_code:claude-opus-5")
    expect(request.effort).toBe("xhigh")
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

  it("keeps a pinned model, and the effort it is run at", () => {
    const body = toUpdateRequest(profileToFormValues(PROFILE))
    expect(body.model).toBe("claude_code:claude-opus-5")
    expect(body.effort).toBe("xhigh")
  })

  it("clears an emptied effort with the same sentinel, which is the CLI's own", () => {
    const values = profileToFormValues({ ...PROFILE, effort: null })
    expect(toUpdateRequest(values).effort).toBe("default")
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
