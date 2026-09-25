/**
 * Friendly aliases for the schemas in the generated `schema.d.ts`.
 *
 * Import DTOs from here rather than reaching into `components["schemas"][...]`
 * everywhere; the generated file stays the single source of truth.
 */

import type { components } from "./schema"

export type { components, operations, paths } from "./schema"

type Schemas = components["schemas"]

export type GoalDto = Schemas["GoalDto"]
export type GoalUsage = Schemas["GoalUsageDto"]
export type GoalStatus = Schemas["GoalStatus"]
export type CreateGoalRequest = Schemas["CreateGoalRequest"]

export type RepositoryDto = Schemas["RepositoryDto"]
export type CreateRepositoryRequest = Schemas["CreateRepositoryRequest"]
export type UpdateRepositoryRequest = Schemas["UpdateRepositoryRequest"]

export type TaskDto = Schemas["TaskDto"]
export type TaskAgentDto = Schemas["TaskAgentDto"]
export type AgentAssignment = Schemas["AgentAssignment"]
export type TaskUsage = Schemas["TaskUsageDto"]
export type TaskStatus = Schemas["TaskStatus"]
export type TaskTransitionDto = Schemas["TaskTransitionDto"]
export type CreateTaskRequest = Schemas["CreateTaskRequest"]
export type UpdateTaskRequest = Schemas["UpdateTaskRequest"]

export type SessionDto = Schemas["SessionDto"]
export type SessionStatus = Schemas["SessionStatus"]
export type AttentionReason = Schemas["AttentionReason"]
export type SessionEntryDto = Schemas["SessionEntryDto"]
export type ResumeOutsideSessionRequest = Schemas["ResumeOutsideSessionRequest"]
export type NewSessionRequest = Schemas["NewSessionRequest"]

/**
 * What one agent spent — the same three counters wherever they are read: a
 * session's own, each half of a task's {@link TaskUsage}, each seat of a
 * goal's {@link GoalUsage}.
 */
export type TokenUsage = Schemas["TokenUsageDto"]

export type SkillDto = Schemas["SkillDto"]
export type CreateSkillRequest = Schemas["CreateSkillRequest"]
export type UpdateSkillRequest = Schemas["UpdateSkillRequest"]
export type Seat = Schemas["Seat"]
export type Landing = Schemas["Landing"]
export type Actor = Schemas["Actor"]
export type MessageDto = Schemas["MessageDto"]
export type MessageKind = Schemas["MessageKind"]
export type ModelDto = Schemas["ModelDto"]
export type ModelRank = Schemas["ModelRank"]
export type EffortDto = Schemas["EffortDto"]
export type SetModelEnabledRequest = Schemas["SetModelEnabledRequest"]
export type SetModelRankRequest = Schemas["SetModelRankRequest"]

export type AgentConfigDto = Schemas["AgentConfigDto"]

export type LogLineDto = Schemas["LogLineDto"]

export type AgentEventDto = Schemas["AgentEventDto"]
export type ResyncDto = Schemas["ResyncDto"]
export type HeartbeatDto = Schemas["HeartbeatDto"]

/**
 * Every domain event carried by `GET /v1/events/stream`, as a tagged union.
 *
 * Minus the kinds starting `memo`: the generated schema still lists them and
 * the app no longer reads them. Drop the `Exclude` when the schema is
 * regenerated without them.
 */
export type DomainEvent = Exclude<Schemas["DomainEvent"], { event: `memo${string}` }>
/** `"goal_updated" | "task_updated" | ...` */
export type DomainEventKind = DomainEvent["event"]
