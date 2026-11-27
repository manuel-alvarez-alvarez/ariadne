/**
 * Trajectory DTOs for the turn console's tests, as the daemon sends them
 * (040): a turn, its steps, a snapshot and an update.
 */

import type { StepDto, TrajectorySnapshotDto, TrajectoryUpdateDto, TurnDto } from "@/api"

let next = 0

/** A fresh id that sorts after every id made before it, as the daemon's event ids do. */
export function anId(): string {
  next += 1
  return `01TRAJ${String(next).padStart(20, "0")}`
}

export function text(value: string) {
  return [{ type: "text" as const, text: value }]
}

export function aTurn(overrides: Partial<TurnDto> = {}): TurnDto {
  const id = overrides.id ?? anId()
  return {
    id,
    lane: "main",
    state: "completed",
    started_at: "2026-11-14T10:00:00Z",
    ended_at: "2026-11-14T10:01:04Z",
    prompt: { id, content: text("Fix the bug"), source: "console" },
    answer: text("Fixed it"),
    answer_step_id: null,
    activity: {
      steps: 5,
      files_changed: 2,
      failed: 1,
      cancelled: 0,
      subagents: 0,
      thoughts: 0,
      tool_calls: 5,
    },
    attention: [],
    attention_more: 0,
    notices: [],
    notices_more: 0,
    earlier_steps: 0,
    steps: [],
    ...overrides,
  }
}

type ToolStep = Extract<StepDto, { type: "tool_call" }>
type PermissionStep = Extract<StepDto, { type: "permission" }>
type QuestionStep = Extract<StepDto, { type: "question" }>
type ErrorStep = Extract<StepDto, { type: "error" }>
type MessageStep = Extract<StepDto, { type: "message" }>
type SubagentStep = Extract<StepDto, { type: "subagent" }>

export function aToolStep(turnId: string, overrides: Partial<ToolStep> = {}): ToolStep {
  return {
    type: "tool_call",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:05Z",
    started_at: "2026-11-14T10:00:05Z",
    ended_at: "2026-11-14T10:00:17Z",
    tool_call_id: anId(),
    kind: "execute",
    status: "completed",
    title: "Run the tests",
    subject: "npm test",
    detail_rev: 1,
    locations: [],
    ...overrides,
  }
}

export function aMessageStep(turnId: string, overrides: Partial<MessageStep> = {}): MessageStep {
  return {
    type: "message",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:20Z",
    content: text("Working on it"),
    streaming: true,
    ...overrides,
  }
}

export function aPermissionStep(
  turnId: string,
  overrides: Partial<PermissionStep> = {},
): PermissionStep {
  return {
    type: "permission",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:30Z",
    tool_call_id: anId(),
    question: "Allow the command?",
    state: "pending",
    tool: { title: "Bash", subject: "rm -rf build", kind: "execute" },
    options: [
      { id: "allow", name: "Allow once", kind: "allow_once" },
      { id: "always", name: "Allow always", kind: "allow_always" },
      { id: "reject", name: "Reject", kind: "reject_once" },
    ],
    ...overrides,
  }
}

export function aQuestionStep(turnId: string, overrides: Partial<QuestionStep> = {}): QuestionStep {
  return {
    type: "question",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:40Z",
    request_id: "01QUESTION0000000000000001",
    hint: "Database",
    question: "Which database?",
    options: [{ id: "pg", label: "Postgres" }],
    state: "open",
    ...overrides,
  }
}

export function anErrorStep(turnId: string, overrides: Partial<ErrorStep> = {}): ErrorStep {
  return {
    type: "error",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:50Z",
    text: "The agent crashed",
    ...overrides,
  }
}

export function aSubagentStep(turnId: string, overrides: Partial<SubagentStep> = {}): SubagentStep {
  return {
    type: "subagent",
    id: anId(),
    turn_id: turnId,
    lane: "main",
    created_at: "2026-11-14T10:00:06Z",
    started_at: "2026-11-14T10:00:06Z",
    ended_at: null,
    subagent_id: "01SUBAGENT0000000000000001",
    title: "Explore the tests",
    task: "Find the flaky test",
    state: "running",
    can_cancel: true,
    activity: {
      steps: 4,
      files_changed: 0,
      failed: 0,
      cancelled: 0,
      subagents: 0,
      thoughts: 0,
      tool_calls: 4,
    },
    ...overrides,
  }
}

export function aSnapshot(
  turns: TurnDto[],
  overrides: Partial<TrajectorySnapshotDto> = {},
): TrajectorySnapshotDto {
  return {
    rev: 1,
    session: {
      status: "idle",
      commands: [],
      config_options: [],
      usage: { input_tokens: 0, output_tokens: 0, cached_input_tokens: 0 },
    },
    turns,
    has_older: false,
    plans: [],
    subagents: [],
    ...overrides,
  }
}

export function anUpdate(rev: number, ops: TrajectoryUpdateDto["ops"]): TrajectoryUpdateDto {
  return { rev, ops }
}
