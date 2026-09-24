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
  components,
  EffortDto,
  GoalDto,
  ModelDto,
  RepositoryDto,
  SessionDto,
  SessionEntryDto,
  SkillDto,
  TaskDto,
} from "@/api"
import type { OutsideSessionDto } from "@/features/sessions/queries"

type SessionPageDto = components["schemas"]["SessionPageDto"]

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
    orchestrated: true,
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
    context_used: null,
    context_size: null,
    created_at: STAMP,
    ended_at: null,
    ...overrides,
  }
}

/**
 * A stored session an ACP agent holds on its own, listed over
 * `GET /v1/sessions?kind=outside` — Ariadne did not start it, and it carries
 * no goal, task or seat, having none of its own.
 */
export function anOutsideSession(overrides: Partial<OutsideSessionDto> = {}): OutsideSessionDto {
  return {
    agent_id: "claude-agent-acp",
    internal_session_id: "outside-internal-id-1",
    working_directory: "/Users/me/dev/ariadne",
    last_activity_at: STAMP,
    first_prompt: "Fix the flaky test.",
    ...overrides,
  }
}

/**
 * One page of `GET /v1/sessions` holding these Ariadne sessions, as the
 * daemon answers it: every one a listing row of kind `ariadne`.
 */
export function aSessionPage(
  sessions: SessionDto[],
  page: Partial<SessionPageDto> = {},
): SessionPageDto {
  return aPage(
    sessions.map(
      ({ worktree_path, ...session }): SessionEntryDto => ({
        ...session,
        kind: "ariadne",
        agent_id: session.model.split(":")[0] ?? "",
        title: null,
        working_directory: worktree_path,
      }),
    ),
    page,
  )
}

/**
 * One page of `GET /v1/sessions?kind=outside` holding these conversations,
 * as the daemon answers it.
 */
export function anOutsideSessionPage(
  sessions: OutsideSessionDto[],
  page: Partial<SessionPageDto> = {},
): SessionPageDto {
  return aPage(
    sessions.map(
      (session): SessionEntryDto => ({
        kind: "outside",
        id: session.internal_session_id,
        agent_id: session.agent_id,
        internal_session_id: session.internal_session_id,
        working_directory: session.working_directory,
        last_activity_at: session.last_activity_at,
        title: session.first_prompt,
      }),
    ),
    page,
  )
}

function aPage(sessions: SessionEntryDto[], page: Partial<SessionPageDto>): SessionPageDto {
  return {
    sessions,
    next_cursor: null,
    total: sessions.length,
    snapshot_at: STAMP,
    ...page,
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
    rank: null,
    ...overrides,
  }
}
