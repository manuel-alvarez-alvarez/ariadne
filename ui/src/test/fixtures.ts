/**
 * The daemon's rows, as a test needs one of them.
 *
 * A `TaskDto` is fifteen fields and a test cares about two, so every test file
 * was writing out the other thirteen — and picking its own ids, its own
 * timestamps and its own defaults doing it. Each builder here answers a
 * complete, valid row, and takes an overlay of whatever the test is actually
 * about: `aTask({ status: "approved" })`.
 *
 * One timestamp for everything, because a test that cares about time says so by
 * overriding it, and one id per kind, because a test that needs two rows apart
 * says that by overriding the id too.
 */

import type {
  AgentConfigDto,
  AgentEventDto,
  EffortDto,
  GoalDto,
  MemoryDto,
  ModelDto,
  RepositoryDto,
  SessionDto,
  SkillDto,
  TaskDto,
} from "@/api"

/** The instant everything the daemon holds was created and last touched. */
const STAMP = "2026-01-01T00:00:00Z"

const GOAL_ID = "01JGOAL0000000000000000001"
const TASK_ID = "01JTASK0000000000000000001"
const SESSION_ID = "01JSESS0000000000000000001"
const AUTHOR_ID = "01JAGENT0000000000000AUTH"
const REVIEWER_ID = "01JAGENT0000000000000REVW"
const REPO_ID = "01JREPO0000000000000000001"

/** A row nobody has reported tokens for, which is how every fixture starts. */
const NO_TOKENS = { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 }

export function aGoal(overrides: Partial<GoalDto> = {}): GoalDto {
  return {
    id: GOAL_ID,
    title: "Ship the board",
    description: "",
    model: "claude-agent-acp:claude-sonnet-5",
    repos: [],
    status: "active",
    usage: {
      total: NO_TOKENS,
      orchestrator: NO_TOKENS,
      authors: NO_TOKENS,
      reviewers: NO_TOKENS,
    },
    created_at: STAMP,
    updated_at: STAMP,
    ...overrides,
  }
}

export function aTask(overrides: Partial<TaskDto> = {}): TaskDto {
  return {
    id: TASK_ID,
    goal_id: GOAL_ID,
    title: "Wire the sessions screen",
    description: "",
    status: "in_progress",
    branch: "wire-the-sessions-screen-000001",
    landing: "merge",
    repo_id: REPO_ID,
    stalled: false,
    agents: [
      {
        id: AUTHOR_ID,
        seat: "author",
        skills: ["coding"],
        model: "claude-agent-acp:claude-sonnet-5",
      },
      {
        id: REVIEWER_ID,
        seat: "reviewer",
        skills: ["code-review"],
        model: "claude-agent-acp:claude-sonnet-5",
      },
    ],
    depends_on: [],
    picks: [],
    usage: { total: NO_TOKENS, author: NO_TOKENS, reviewers: [] },
    created_at: STAMP,
    updated_at: STAMP,
    ...overrides,
  }
}

export function aSession(overrides: Partial<SessionDto> = {}): SessionDto {
  const id = overrides.id ?? SESSION_ID
  return {
    id,
    goal_id: GOAL_ID,
    task_id: TASK_ID,
    seat: "author",
    task_agent_id: AUTHOR_ID,
    model: "claude-agent-acp:claude-sonnet-5",
    internal_session_id: null,
    worktree_path: null,
    status: "running",
    attention_reason: null,
    attention_since: null,
    last_activity_at: STAMP,
    usage: NO_TOKENS,
    created_at: STAMP,
    ended_at: null,
    ...overrides,
  }
}

export function anAgentEvent(overrides: Partial<AgentEventDto> = {}): AgentEventDto {
  return {
    id: "01JEVENT000000000000000001",
    session_id: SESSION_ID,
    task_id: TASK_ID,
    kind: "post_tool_use",
    summary: "Read AGENTS.md.",
    payload: { tool_name: "Read", cwd: "/Users/me/dev/ariadne" },
    created_at: STAMP,
    ...overrides,
  }
}

/**
 * The console's own event kinds (008, 021), each as the runtime reports it.
 * The live-only three — a message chunk, a thought chunk, a tool call's
 * progress — reach a console stream and nothing else; the rest are stored.
 */
export function aMessageChunk(text: string, id: string): AgentEventDto {
  return anAgentEvent({ id, kind: "agent_message_chunk", summary: text, payload: { text } })
}

export function aThoughtChunk(text: string, id: string): AgentEventDto {
  return anAgentEvent({ id, kind: "agent_thought_chunk", summary: text, payload: { text } })
}

/** A tool call as `pre_tool_use`, `tool_call_update` and `post_tool_use` carry it. */
export function aToolCall(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    toolCallId: "call-1",
    title: "Bash",
    kind: "execute",
    status: "pending",
    rawInput: { command: "ls" },
    ...overrides,
  }
}

export function aToolEvent(
  kind: "pre_tool_use" | "tool_call_update" | "post_tool_use",
  call: Record<string, unknown>,
  id: string,
): AgentEventDto {
  return anAgentEvent({
    id,
    kind,
    summary: String(call.title ?? call.toolCallId),
    payload: {
      tool_name: call.title,
      tool_input: call.rawInput,
      tool_call_id: call.toolCallId,
      acp: call,
    },
  })
}

export function aPlan(entries: Record<string, unknown>[], id: string): AgentEventDto {
  return anAgentEvent({ id, kind: "plan", summary: "Plan updated.", payload: { entries } })
}

export function aStop(stopReason: string, id: string): AgentEventDto {
  return anAgentEvent({
    id,
    kind: "stop",
    summary: `Turn ended: ${stopReason}.`,
    payload: { stop_reason: stopReason },
  })
}

export function aSkill(overrides: Partial<SkillDto> = {}): SkillDto {
  return {
    name: "coding",
    seat: "task",
    summary: "Implement a task from its specification.",
    document: "---\nname: coding\ndescription: Implement a task from its specification.\n---\n",
    document_is_default: true,
    builtin: true,
    created_at: STAMP,
    updated_at: STAMP,
    ...overrides,
  }
}

export function aRepository(overrides: Partial<RepositoryDto> = {}): RepositoryDto {
  return {
    id: REPO_ID,
    path: "/home/me/dev/ariadne",
    base_branch: "main",
    description: "The orchestrator itself.",
    created_at: STAMP,
    updated_at: STAMP,
    ...overrides,
  }
}

export function aMemory(overrides: Partial<MemoryDto> = {}): MemoryDto {
  return {
    id: "01JMEM00000000000000ONE1",
    repository_id: REPO_ID,
    text: "The lint config lives in biome.json, not .eslintrc.",
    source_session_id: SESSION_ID,
    source_task_id: TASK_ID,
    source_goal_id: GOAL_ID,
    created_at: STAMP,
    // Far enough out that a real clock never treats the fixture as expired.
    expires_at: "2099-01-01T00:00:00Z",
    ...overrides,
  }
}

export function anAgentConfig(overrides: Partial<AgentConfigDto> = {}): AgentConfigDto {
  return {
    agent_id: "claude-agent-acp",
    extra_flags: [],
    default_flags: [],
    ...overrides,
  }
}

/** One reasoning effort a `GET /v1/models` entry can be run at. */
export function anEffort(overrides: Partial<EffortDto> = {}): EffortDto {
  return {
    id: "medium",
    description: null,
    default: false,
    ...overrides,
  }
}

/** One entry of the model catalog `GET /v1/models` serves. */
export function aModel(overrides: Partial<ModelDto> = {}): ModelDto {
  return {
    id: "claude-agent-acp:claude-sonnet-5",
    agent_id: "claude-agent-acp",
    description: null,
    efforts: [],
    enabled: true,
    ...overrides,
  }
}
