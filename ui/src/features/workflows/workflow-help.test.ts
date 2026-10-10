import { describe, expect, it } from "vitest"

import { workflowCompletionsAt, workflowHoverAt } from "./workflow-help"

const skills = [
  { name: "coding", summary: "Build the task whole and prove it with tests." },
  { name: "code-review", summary: "Review the diff for correctness." },
  { name: "merge", summary: "Rebase, run the suite and land the branch." },
  { name: "orchestration", summary: "The orchestrator's own skill." },
  { name: "pr-reviewer", summary: "What a reviewer pull request session loads." },
]

/** Cursor sits right after `before`; `after` is whatever follows it on the line. */
function completionsAt(before: string, after = "", firstLine = false) {
  return workflowCompletionsAt(before + after, before.length, firstLine, skills)
}

describe("workflowCompletionsAt", () => {
  it("offers the body keys at the start of a body line", () => {
    const match = completionsAt("    ")
    expect(match?.options.map((o) => o.label)).toEqual(["skills:", "rank:", "gate:"])
  })

  it("filters the body keys by what is already typed", () => {
    const match = completionsAt("    ra")
    expect(match?.options.map((o) => o.label)).toEqual(["rank:"])
  })

  it("offers nothing on the workflow line", () => {
    expect(completionsAt("", "", true)).toBeNull()
  })

  it("offers the rank words after rank:", () => {
    const match = completionsAt("    rank: ")
    expect(match?.options.map((o) => o.label)).toEqual(["fast", "balanced", "frontier", "local"])
  })

  it("filters rank words by what is already typed", () => {
    const match = completionsAt("    rank: fa")
    expect(match?.options.map((o) => o.label)).toEqual(["fast"])
  })

  it("offers the gate words after gate:", () => {
    const match = completionsAt("    gate: ")
    expect(match?.options.map((o) => o.label)).toEqual([
      "committed",
      "pushed",
      "merged",
      "request-merged",
    ])
  })

  it("offers skill names after skills:, with each summary as the detail", () => {
    const match = completionsAt("    skills: ")
    expect(match?.options).toEqual([
      { label: "coding", detail: skills[0]?.summary },
      { label: "code-review", detail: skills[1]?.summary },
      { label: "merge", detail: skills[2]?.summary },
    ])
  })

  it("never offers orchestration or pr-reviewer", () => {
    const match = completionsAt("    skills: ")
    const labels = match?.options.map((o) => o.label) ?? []
    expect(labels).not.toContain("orchestration")
    expect(labels).not.toContain("pr-reviewer")
  })

  it("offers skill names after a comma, excluding the one the line already names", () => {
    const match = completionsAt("    skills: coding, ")
    const labels = match?.options.map((o) => o.label) ?? []
    expect(labels).not.toContain("coding")
    expect(labels).toContain("code-review")
  })

  it("filters skill names by the partial word after the comma", () => {
    const match = completionsAt("    skills: coding, cod")
    expect(match?.options.map((o) => o.label)).toEqual(["code-review"])
  })

  it("excludes a skill already named later on the line, inserting before it", () => {
    const match = completionsAt("    skills: ", ", coding")
    const labels = match?.options.map((o) => o.label) ?? []
    expect(labels).not.toContain("coding")
    expect(labels).toContain("code-review")
  })

  it("excludes skills named on both sides, inserting between them", () => {
    const match = completionsAt("    skills: coding, ", ", merge")
    const labels = match?.options.map((o) => o.label) ?? []
    expect(labels).not.toContain("coding")
    expect(labels).not.toContain("merge")
    expect(labels).toContain("code-review")
  })

  it("offers nothing past a description line", () => {
    expect(completionsAt("    Build the task")).toBeNull()
  })
})

describe("workflowHoverAt", () => {
  it("explains the skills key", () => {
    const line = "    skills: coding"
    const match = workflowHoverAt(line, line.indexOf("skills"), false, skills)
    expect(match?.text).toBe(
      "Names the skills this column's agent loads, comma-separated, from the skill catalog.",
    )
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

  it.each([
    ["frontier", "Frontier means the strongest model, when a task earns it."],
    ["balanced", "Balanced means the default for most tasks."],
    ["fast", "Fast means the cheapest model that still earns a task."],
    ["local", "Local means off the ladder, staffed only where a task names it."],
  ])("explains the rank value %s", (value, text) => {
    const line = `    rank: ${value}`
    const match = workflowHoverAt(line, line.indexOf(value), false, skills)
    expect(match?.text).toBe(text)
  })

  it.each([
    ["committed", "Committed means at least one commit past the base, with a clean worktree."],
    ["pushed", "Pushed means the remote holds the branch tip."],
    [
      "merged",
      "Merged means the supplied merge commit and the task branch are ancestors of the base branch.",
    ],
    ["request-merged", "Request-merged means a fresh forge read says the task's request merged."],
  ])("explains the gate value %s", (value, text) => {
    const line = `    gate: ${value}`
    const match = workflowHoverAt(line, line.indexOf(value), false, skills)
    expect(match?.text).toBe(text)
  })

  it("explains a rank value directly after the colon, with no space", () => {
    const line = "    rank:fast"
    const match = workflowHoverAt(line, line.indexOf("fast"), false, skills)
    expect(match?.text).toBe("Fast means the cheapest model that still earns a task.")
  })

  it("explains a gate value directly after the colon, with no space", () => {
    const line = "    gate:merged"
    const match = workflowHoverAt(line, line.indexOf("merged"), false, skills)
    expect(match?.text).toBe(
      "Merged means the supplied merge commit and the task branch are ancestors of the base branch.",
    )
  })

  it("explains a skill name directly after the colon, with no space", () => {
    const line = "    skills:coding"
    const match = workflowHoverAt(line, line.indexOf("coding"), false, skills)
    expect(match?.text).toBe(skills[0]?.summary)
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
