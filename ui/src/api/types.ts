/**
 * Friendly aliases for the schemas in the generated `schema.d.ts`.
 *
 * Import DTOs from here rather than reaching into `components["schemas"][...]`
 * everywhere; the generated file stays the single source of truth.
 */

import type { components, paths } from "./schema"

export type { components, operations, paths } from "./schema"

type Schemas = components["schemas"]

export type ApiPaths = keyof paths

export type HealthResponse = Schemas["HealthResponse"]
export type VersionResponse = Schemas["VersionResponse"]

export type GoalDto = Schemas["GoalDto"]
export type GoalStatus = Schemas["GoalStatus"]
export type CreateGoalRequest = Schemas["CreateGoalRequest"]
export type FinalizePlanRequest = Schemas["FinalizePlanRequest"]

export type RepositoryDto = Schemas["RepositoryDto"]
export type CreateRepositoryRequest = Schemas["CreateRepositoryRequest"]
export type UpdateRepositoryRequest = Schemas["UpdateRepositoryRequest"]

export type TaskDto = Schemas["TaskDto"]
export type TaskStatus = Schemas["TaskStatus"]
export type TaskReviewerDto = Schemas["TaskReviewerDto"]
export type TaskTransitionDto = Schemas["TaskTransitionDto"]
export type TaskUpdatedDto = Schemas["TaskUpdatedDto"]
export type CreateTaskRequest = Schemas["CreateTaskRequest"]
export type UpdateTaskRequest = Schemas["UpdateTaskRequest"]
export type TransitionRequest = Schemas["TransitionRequest"]

export type MessageDto = Schemas["MessageDto"]
export type MessageRecipientDto = Schemas["MessageRecipientDto"]
export type CreateMessageRequest = Schemas["CreateMessageRequest"]
export type AuthorRole = Schemas["AuthorRole"]

export type ReviewDto = Schemas["ReviewDto"]
export type ReviewVerdict = Schemas["ReviewVerdict"]
export type CreateReviewRequest = Schemas["CreateReviewRequest"]

export type SessionDto = Schemas["SessionDto"]
export type SessionStatus = Schemas["SessionStatus"]
export type AttentionReason = Schemas["AttentionReason"]
export type SessionLogsResponse = Schemas["SessionLogsResponse"]

export type ProfileDto = Schemas["ProfileDto"]
export type CreateProfileRequest = Schemas["CreateProfileRequest"]
export type UpdateProfileRequest = Schemas["UpdateProfileRequest"]
export type ProfilePromptDto = Schemas["ProfilePromptDto"]
export type PromptDefaultDto = Schemas["PromptDefaultDto"]
export type RolePromptDefaultsDto = Schemas["RolePromptDefaultsDto"]
export type NewProfilePrompt = Schemas["NewProfilePrompt"]
export type PromptKind = Schemas["PromptKind"]
export type UpdateProfilePromptRequest = Schemas["UpdateProfilePromptRequest"]
export type Role = Schemas["Role"]
export type AgentKind = Schemas["AgentKind"]
export type ModelDto = Schemas["ModelDto"]

export type AgentConfigDto = Schemas["AgentConfigDto"]
export type UpdateAgentConfigRequest = Schemas["UpdateAgentConfigRequest"]

export type LogLineDto = Schemas["LogLineDto"]
export type LogSnapshotResponse = Schemas["LogSnapshotResponse"]

export type AgentEventDto = Schemas["AgentEventDto"]
export type DeletedDto = Schemas["DeletedDto"]
export type ResyncDto = Schemas["ResyncDto"]

/** Every domain event carried by `GET /v1/events/stream`, as a tagged union. */
export type DomainEvent = Schemas["DomainEvent"]
/** `"goal_updated" | "task_updated" | ...` */
export type DomainEventKind = DomainEvent["event"]
/** The payload of one event kind, e.g. `DomainEventOf<"task_updated">`. */
export type DomainEventOf<K extends DomainEventKind> = Extract<DomainEvent, { event: K }>["data"]
