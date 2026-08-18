/**
 * Tests for the one thing in this feature that is easy to get subtly wrong: the
 * daemon's clearing sentinels.
 *
 * `UpdateProfileRequest` clears the agent kind with the string `"auto"` and the
 * model with `"default"`, while `CreateProfileRequest` takes plain nulls for the
 * same two states. Getting that backwards writes a profile pinned to a model
 * literally named "default", which nothing else in the stack would catch.
 */

import { describe, expect, it } from "vitest"

import type { ProfileDto } from "@/api"

import {
  emptyProfileFormValues,
  profileToFormValues,
  toCreateRequest,
  toUpdateRequest,
} from "./profile-form-values"

const PROFILE: ProfileDto = {
  id: "p1",
  name: "rust-engineer",
  role: "engineer",
  agent_kind: "claude_code",
  model: "claude-opus-5",
  system_prompt: "You are a Rust engineer.",
  extra_flags: ["--permission-mode=acceptEdits"],
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
      extraFlags: [{ value: "--permission-mode=acceptEdits" }],
    })
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
      extra_flags: [],
    })
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

describe("extra flags", () => {
  it("trims rows and drops the blank ones", () => {
    const values = {
      ...profileToFormValues(PROFILE),
      extraFlags: [{ value: "  --verbose  " }, { value: "   " }, { value: "--flag=1" }],
    }
    expect(toCreateRequest(values).extra_flags).toEqual(["--verbose", "--flag=1"])
    expect(toUpdateRequest(values).extra_flags).toEqual(["--verbose", "--flag=1"])
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
