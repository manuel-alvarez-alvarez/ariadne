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
  GoalDto,
  MessageDto,
  ProfileDto,
  RepositoryDto,
  SessionDto,
  TaskDto,
} from "@/api"

/** The instant everything the daemon holds was created and last touched. */
const STAMP = "2026-01-01T00:00:00Z"

const GOAL_ID = "01JGOAL0000000000000000001"
const TASK_ID = "01JTASK0000000000000000001"
const SESSION_ID = "01JSESS0000000000000000001"
const PROFILE_ID = "01JPROF00000000000000ENGI"
const REPO_ID = "01JREPO0000000000000000001"

/** A row nobody has reported tokens for, which is how every fixture starts. */
const NO_TOKENS = { input_tokens: 0, cached_input_tokens: 0, output_tokens: 0 }

export function aGoal(overrides: Partial<GoalDto> = {}): GoalDto {
  return {
    id: GOAL_ID,
    title: "Ship the board",
    description: "",
    planner_profile_id: "01JPROF00000000000000PLAN",
    repos: [],
    required_approvals: 1,
    status: "active",
    usage: {
      total: NO_TOKENS,
      planner: NO_TOKENS,
      engineers: NO_TOKENS,
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
    repo_id: REPO_ID,
    stalled: false,
    engineer_profile_id: PROFILE_ID,
    reviewers: [],
    depends_on: [],
    review_round: 0,
    usage: { total: NO_TOKENS, engineer: NO_TOKENS, reviewers: [] },
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
    role: "engineer",
    profile_id: PROFILE_ID,
    agent_kind: "claude_code",
    model: null,
    internal_session_id: null,
    tmux_session: `ariadne-${id}`,
    worktree_path: null,
    review_round: null,
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

export function aProfile(overrides: Partial<ProfileDto> = {}): ProfileDto {
  return {
    id: PROFILE_ID,
    name: "Engineer",
    role: "engineer",
    model: null,
    system_prompt: "",
    system_prompt_is_default: false,
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
    merge_strategy: "direct",
    description: "The orchestrator itself.",
    created_at: STAMP,
    updated_at: STAMP,
    ...overrides,
  }
}

export function aMessage(overrides: Partial<MessageDto> = {}): MessageDto {
  return {
    id: "01JMESG0000000000000000001",
    goal_id: GOAL_ID,
    task_id: null,
    author_role: "user",
    author_session_id: null,
    body: "",
    created_at: STAMP,
    ...overrides,
  }
}

export function anAgentConfig(overrides: Partial<AgentConfigDto> = {}): AgentConfigDto {
  return {
    agent_kind: "claude_code",
    extra_flags: [],
    default_flags: [],
    ...overrides,
  }
}
