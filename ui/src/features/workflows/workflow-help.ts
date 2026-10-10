/**
 * The autocomplete and hover-help logic for the workflow document: a column's
 * key line (`skills:`, `rank:`, `gate:`), a rank or gate value, and a skill
 * name. Kept pure and independent of CodeMirror so each rule is a function on
 * plain text, provable without a browser; `workflow-help-extensions.ts` is
 * the thin adapter that wires these into the editor.
 */

export interface SkillOption {
  name: string
  summary: string
}

interface WorkflowCompletionOption {
  label: string
  detail?: string
}

interface WorkflowCompletionMatch {
  from: number
  options: WorkflowCompletionOption[]
}

interface WorkflowHoverMatch {
  from: number
  to: number
  text: string
}

const KEYS = ["skills", "rank", "gate"] as const
const RANK_VALUES = ["fast", "balanced", "frontier", "local"] as const
const GATE_VALUES = ["committed", "pushed", "merged", "request-merged"] as const

/** Neither staffs a task agent, so a save refuses them in `skills:` (030 rule 3). */
const FORBIDDEN_SKILLS = new Set(["orchestration", "pr-reviewer"])

const KEY_HELP: Record<(typeof KEYS)[number], string> = {
  skills: "Names the skills this column's agent loads, comma-separated, from the skill catalog.",
  rank: "Names the model rank for this column: fast, balanced, frontier or local.",
  gate: "Names the work state required before this column completes: committed, pushed, merged or request-merged.",
}

const RANK_HELP: Record<(typeof RANK_VALUES)[number], string> = {
  frontier: "Frontier means the strongest model, when a task earns it.",
  balanced: "Balanced means the default for most tasks.",
  fast: "Fast means the cheapest model that still earns a task.",
  local: "Local means off the ladder, staffed only where a task names it.",
}

const GATE_HELP: Record<(typeof GATE_VALUES)[number], string> = {
  committed: "Committed means at least one commit past the base, with a clean worktree.",
  pushed: "Pushed means the remote holds the branch tip.",
  merged:
    "Merged means the supplied merge commit and the task branch are ancestors of the base branch.",
  "request-merged": "Request-merged means a fresh forge read says the task's request merged.",
}

function isRankValue(value: string): value is (typeof RANK_VALUES)[number] {
  return (RANK_VALUES as readonly string[]).includes(value)
}

function isGateValue(value: string): value is (typeof GATE_VALUES)[number] {
  return (GATE_VALUES as readonly string[]).includes(value)
}

function trailingWord(text: string): string {
  return /[a-z0-9-]*$/i.exec(text)?.[0] ?? ""
}

function skillsAlreadyNamed(listText: string): Set<string> {
  const segments = listText.split(",")
  segments.pop()
  return new Set(segments.map((segment) => segment.trim()).filter(Boolean))
}

/**
 * The completions the workflow editor offers at the cursor, given the line's
 * text up to the cursor (`before`) and whether that line is the document's
 * first line (the `workflow <name>` line, where none of this applies).
 */
export function workflowCompletionsAt(
  before: string,
  firstLine: boolean,
  skills: SkillOption[],
): WorkflowCompletionMatch | null {
  if (firstLine) return null
  const partial = trailingWord(before).toLowerCase()
  const from = before.length - partial.length
  const prefix = before.slice(0, from)

  if (/^\s*$/.test(prefix)) {
    const options = KEYS.filter((key) => key.startsWith(partial)).map((key) => ({
      label: `${key}:`,
    }))
    return options.length ? { from, options } : null
  }

  if (/^\s*rank:\s*$/.test(prefix)) {
    const options = RANK_VALUES.filter((value) => value.startsWith(partial)).map((value) => ({
      label: value,
    }))
    return options.length ? { from, options } : null
  }

  if (/^\s*gate:\s*$/.test(prefix)) {
    const options = GATE_VALUES.filter((value) => value.startsWith(partial)).map((value) => ({
      label: value,
    }))
    return options.length ? { from, options } : null
  }

  const skillsList = /^\s*skills:\s*((?:[a-z][a-z0-9-]*\s*,\s*)*)$/i.exec(prefix)
  if (skillsList) {
    const already = skillsAlreadyNamed(skillsList[1] ?? "")
    const options = skills
      .filter((skill) => !FORBIDDEN_SKILLS.has(skill.name))
      .filter((skill) => !already.has(skill.name))
      .filter((skill) => skill.name.toLowerCase().startsWith(partial))
      .map((skill) => ({ label: skill.name, detail: skill.summary }))
    return options.length ? { from, options } : null
  }

  return null
}

/**
 * The hover help at a character offset in a line's text, given whether that
 * line is the document's first line (where none of this applies).
 */
export function workflowHoverAt(
  lineText: string,
  ch: number,
  firstLine: boolean,
  skills: SkillOption[],
): WorkflowHoverMatch | null {
  if (firstLine) return null

  const keyMatch = /^(\s*)(skills|rank|gate):/.exec(lineText)
  if (keyMatch) {
    const indent = keyMatch[1] ?? ""
    const key = (keyMatch[2] ?? "") as (typeof KEYS)[number]
    const from = indent.length
    const to = from + key.length + 1
    if (ch >= from && ch <= to) {
      return { from, to, text: KEY_HELP[key] }
    }
  }

  const rankMatch = /^\s*rank:\s*/.exec(lineText)
  if (rankMatch) {
    const from = rankMatch[0].length
    const value = /^[a-z-]*/i.exec(lineText.slice(from))?.[0] ?? ""
    const to = from + value.length
    if (ch >= from && ch <= to && isRankValue(value)) {
      return { from, to, text: RANK_HELP[value] }
    }
  }

  const gateMatch = /^\s*gate:\s*/.exec(lineText)
  if (gateMatch) {
    const from = gateMatch[0].length
    const value = /^[a-z-]*/i.exec(lineText.slice(from))?.[0] ?? ""
    const to = from + value.length
    if (ch >= from && ch <= to && isGateValue(value)) {
      return { from, to, text: GATE_HELP[value] }
    }
  }

  const skillsLine = /^\s*skills:\s*/.exec(lineText)
  if (skillsLine) {
    const listStart = skillsLine[0].length
    for (const token of lineText.slice(listStart).matchAll(/[a-z][a-z0-9-]*/gi)) {
      const from = listStart + (token.index ?? 0)
      const to = from + token[0].length
      if (ch >= from && ch <= to) {
        const skill = skills.find((candidate) => candidate.name === token[0])
        return { from, to, text: skill ? skill.summary : `No skill is called ${token[0]}` }
      }
    }
  }

  return null
}
