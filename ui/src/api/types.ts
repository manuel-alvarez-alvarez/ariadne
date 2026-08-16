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
export type GoalRepoDto = Schemas["GoalRepoDto"]
export type GoalStatus = Schemas["GoalStatus"]
export type CreateGoalRequest = Schemas["CreateGoalRequest"]
export type RepoSpec = Schemas["RepoSpec"]
export type FinalizePlanRequest = Schemas["FinalizePlanRequest"]

export type TaskDto = Schemas["TaskDto"]
export type TaskStatus = Schemas["TaskStatus"]
export type TaskTransitionDto = Schemas["TaskTransitionDto"]
export type TaskUpdatedDto = Schemas["TaskUpdatedDto"]
export type CreateTaskRequest = Schemas["CreateTaskRequest"]
export type UpdateTaskRequest = Schemas["UpdateTaskRequest"]
export type TransitionRequest = Schemas["TransitionRequest"]

export type MessageDto = Schemas["MessageDto"]
export type CreateMessageRequest = Schemas["CreateMessageRequest"]
export type AuthorRole = Schemas["AuthorRole"]

export type ReviewDto = Schemas["ReviewDto"]
export type ReviewVerdict = Schemas["ReviewVerdict"]
export type CreateReviewRequest = Schemas["CreateReviewRequest"]

export type SessionDto = Schemas["SessionDto"]
export type SessionStatus = Schemas["SessionStatus"]
export type SessionLogsResponse = Schemas["SessionLogsResponse"]

export type ProfileDto = Schemas["ProfileDto"]
export type CreateProfileRequest = Schemas["CreateProfileRequest"]
export type UpdateProfileRequest = Schemas["UpdateProfileRequest"]
export type Role = Schemas["Role"]
export type AgentKind = Schemas["AgentKind"]

export type AgentEventDto = Schemas["AgentEventDto"]
export type DeletedDto = Schemas["DeletedDto"]
export type ResyncDto = Schemas["ResyncDto"]

/** Every domain event carried by `GET /v1/events/stream`, as a tagged union. */
export type DomainEvent = Schemas["DomainEvent"]
/** `"goal_updated" | "task_updated" | ...` */
export type DomainEventKind = DomainEvent["event"]
/** The payload of one event kind, e.g. `DomainEventOf<"task_updated">`. */
export type DomainEventOf<K extends DomainEventKind> = Extract<DomainEvent, { event: K }>["data"]
