import { describe, expect, it } from "vitest"

import { workflowCompletionsAt, workflowHoverAt } from "./workflow-help"

const skills = [
  { name: "coding", summary: "Build the task whole and prove it with tests." },
  { name: "code-review", summary: "Review the diff for correctness." },
  { name: "merge", summary: "Rebase, run the suite and land the branch." },
  { name: "orchestration", summary: "The orchestrator's own skill." },
  { name: "pr-reviewer", summary: "What a reviewer pull request session loads." },
]

describe("workflowCompletionsAt", () => {
  it("offers the body keys at the start of a body line", () => {
    const match = workflowCompletionsAt("    ", false, skills)
    expect(match?.options.map((o) => o.label)).toEqual(["skills:", "rank:", "gate:"])
  })

  it("filters the body keys by what is already typed", () => {
    const match = workflowCompletionsAt("    ra", false, skills)
    expect(match?.options.map((o) => o.label)).toEqual(["rank:"])
  })

  it("offers nothing on the workflow line", () => {
    expect(workflowCompletionsAt("", true, skills)).toBeNull()
  })

  it("offers the rank words after rank:", () => {
    const match = workflowCompletionsAt("    rank: ", false, skills)
    expect(match?.options.map((o) => o.label)).toEqual(["fast", "balanced", "frontier", "local"])
  })

  it("filters rank words by what is already typed", () => {
    const match = workflowCompletionsAt("    rank: fa", false, skills)
    expect(match?.options.map((o) => o.label)).toEqual(["fast"])
  })

  it("offers the gate words after gate:", () => {
    const match = workflowCompletionsAt("    gate: ", false, skills)
    expect(match?.options.map((o) => o.label)).toEqual([
      "committed",
      "pushed",
      "merged",
      "request-merged",
    ])
  })

  it("offers skill names after skills:, with each summary as the detail", () => {
    const match = workflowCompletionsAt("    skills: ", false, skills)
    expect(match?.options).toEqual([
      { label: "coding", detail: skills[0]?.summary },
      { label: "code-review", detail: skills[1]?.summary },
      { label: "merge", detail: skills[2]?.summary },
    ])
  })

  it("never offers orchestration or pr-reviewer", () => {
    const match = workflowCompletionsAt("    skills: ", false, skills)
    const labels = match?.options.map((o) => o.label) ?? []
    expect(labels).not.toContain("orchestration")
    expect(labels).not.toContain("pr-reviewer")
  })

  it("offers skill names after a comma, excluding the one the line already names", () => {
    const match = workflowCompletionsAt("    skills: coding, ", false, skills)
    const labels = match?.options.map((o) => o.label) ?? []
    expect(labels).not.toContain("coding")
    expect(labels).toContain("code-review")
  })

  it("filters skill names by the partial word after the comma", () => {
    const match = workflowCompletionsAt("    skills: coding, cod", false, skills)
    expect(match?.options.map((o) => o.label)).toEqual(["code-review"])
  })

  it("offers nothing past a description line", () => {
    expect(workflowCompletionsAt("    Build the task", false, skills)).toBeNull()
  })
})

describe("workflowHoverAt", () => {
  it("explains the skills key", () => {
    const line = "    skills: coding"
    const match = workflowHoverAt(line, line.indexOf("skills"), false, skills)
    expect(match?.text).toContain("skills")
  })

  it("explains the rank key", () => {
    const line = "    rank: balanced"
    const match = workflowHoverAt(line, line.indexOf("rank"), false, skills)
    expect(match?.text).toContain("fast, balanced, frontier or local")
  })

  it("explains the gate key", () => {
    const line = "    gate: committed"
    const match = workflowHoverAt(line, line.indexOf("gate"), false, skills)
    expect(match?.text).toContain("committed, pushed, merged or request-merged")
  })

  it("explains a rank value", () => {
    const line = "    rank: frontier"
    const match = workflowHoverAt(line, line.indexOf("frontier"), false, skills)
    expect(match?.text).toBe("Frontier means the strongest model, when a task earns it.")
  })

  it("explains a gate value", () => {
    const line = "    gate: request-merged"
    const match = workflowHoverAt(line, line.indexOf("request-merged"), false, skills)
    expect(match?.text).toBe(
      "Request-merged means a fresh forge read says the task's request merged.",
    )
  })

  it("shows a known skill's summary", () => {
    const line = "    skills: coding, code-review"
    const match = workflowHoverAt(line, line.indexOf("code-review"), false, skills)
    expect(match?.text).toBe(skills[1]?.summary)
  })

  it("says when the catalog has no such skill", () => {
    const line = "    skills: made-up"
    const match = workflowHoverAt(line, line.indexOf("made-up"), false, skills)
    expect(match?.text).toBe("No skill is called made-up")
  })

  it("shows nothing on the workflow line", () => {
    const line = "workflow develop-review-merge"
    expect(workflowHoverAt(line, line.indexOf("workflow"), true, skills)).toBeNull()
  })

  it("shows nothing off a key, value or skill name", () => {
    const line = "    Build the task on its branch."
    expect(workflowHoverAt(line, line.indexOf("task"), false, skills)).toBeNull()
  })
})
