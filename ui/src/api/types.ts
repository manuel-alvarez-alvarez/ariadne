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
export type MergeStrategy = Schemas["MergeStrategy"]
export type CreateRepositoryRequest = Schemas["CreateRepositoryRequest"]
export type UpdateRepositoryRequest = Schemas["UpdateRepositoryRequest"]

export type TaskDto = Schemas["TaskDto"]
export type TaskUsage = Schemas["TaskUsageDto"]
export type TaskStatus = Schemas["TaskStatus"]
export type TaskTransitionDto = Schemas["TaskTransitionDto"]
export type CreateTaskRequest = Schemas["CreateTaskRequest"]
export type UpdateTaskRequest = Schemas["UpdateTaskRequest"]

export type MessageDto = Schemas["MessageDto"]
export type MessageRecipientDto = Schemas["MessageRecipientDto"]
export type CreateMessageRequest = Schemas["CreateMessageRequest"]
export type AuthorRole = Schemas["AuthorRole"]

export type ReviewDto = Schemas["ReviewDto"]
export type ReviewVerdict = Schemas["ReviewVerdict"]

export type SessionDto = Schemas["SessionDto"]
export type SessionStatus = Schemas["SessionStatus"]
export type AttentionReason = Schemas["AttentionReason"]

/**
 * What one agent spent — the same three counters wherever they are read: a
 * session's own, each half of a task's {@link TaskUsage}, each role of a
 * goal's {@link GoalUsage}.
 */
export type TokenUsage = Schemas["TokenUsageDto"]

export type ProfileDto = Schemas["ProfileDto"]
export type CreateProfileRequest = Schemas["CreateProfileRequest"]
export type UpdateProfileRequest = Schemas["UpdateProfileRequest"]
export type ProfilePromptDto = Schemas["ProfilePromptDto"]
export type PromptKind = Schemas["PromptKind"]
export type Role = Schemas["Role"]
export type AgentKind = Schemas["AgentKind"]
export type ModelDto = Schemas["ModelDto"]

export type AgentConfigDto = Schemas["AgentConfigDto"]

export type LogLineDto = Schemas["LogLineDto"]

export type AgentEventDto = Schemas["AgentEventDto"]
export type ResyncDto = Schemas["ResyncDto"]

/** Every domain event carried by `GET /v1/events/stream`, as a tagged union. */
export type DomainEvent = Schemas["DomainEvent"]
/** `"goal_updated" | "task_updated" | ...` */
export type DomainEventKind = DomainEvent["event"]
