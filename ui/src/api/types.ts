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
export type IssueDto = Schemas["IssueDto"]
export type GoalUsage = Schemas["GoalUsageDto"]
export type GoalStatus = Schemas["GoalStatus"]
export type CreateGoalRequest = Schemas["CreateGoalRequest"]

export type RepositoryDto = Schemas["RepositoryDto"]
export type CreateRepositoryRequest = Schemas["CreateRepositoryRequest"]
export type UpdateRepositoryRequest = Schemas["UpdateRepositoryRequest"]
export type ForgeDto = Schemas["ForgeDto"]
export type ForgeTunnelDto = Schemas["ForgeTunnelDto"]
export type SetTunnelRequest = Schemas["SetTunnelRequest"]
export type PermissionMode = Schemas["PermissionMode"]

export type TaskDto = Schemas["TaskDto"]
export type AgentAssignment = Schemas["AgentAssignment"]
export type TaskUsage = Schemas["TaskUsageDto"]
export type TaskStatus = Schemas["TaskStatus"]
export type TaskTransitionDto = Schemas["TaskTransitionDto"]
export type MessageDto = Schemas["MessageDto"]
export type CreateTaskRequest = Schemas["CreateTaskRequest"]
export type UpdateTaskRequest = Schemas["UpdateTaskRequest"]

export type SessionDto = Schemas["SessionDto"]
export type SessionStatus = Schemas["SessionStatus"]
export type AttentionReason = Schemas["AttentionReason"]
export type SessionEntryDto = Schemas["SessionEntryDto"]
export type ResumeOutsideSessionRequest = Schemas["ResumeOutsideSessionRequest"]
export type NewSessionRequest = Schemas["NewSessionRequest"]
export type SwitchSessionRequest = Schemas["SwitchSessionRequest"]

/**
 * What one agent spent — the same three counters wherever they are read: a
 * session's own, each half of a task's {@link TaskUsage}, each seat of a
 * goal's {@link GoalUsage}.
 */
export type TokenUsage = Schemas["TokenUsageDto"]

export type SkillDto = Schemas["SkillDto"]
export type CreateSkillRequest = Schemas["CreateSkillRequest"]
export type UpdateSkillRequest = Schemas["UpdateSkillRequest"]
export type WorkflowDto = Schemas["WorkflowDto"]
export type WorkflowStepDto = Schemas["WorkflowStepDto"]
export type ParsedWorkflowDto = Schemas["ParsedWorkflowDto"]
export type CreateWorkflowRequest = Schemas["CreateWorkflowRequest"]
export type UpdateWorkflowRequest = Schemas["UpdateWorkflowRequest"]
export type Seat = Schemas["Seat"]
export type ModelDto = Schemas["ModelDto"]
export type WorkStatsDto = Schemas["WorkStatsDto"]
export type WorkBucketDto = Schemas["WorkBucketDto"]
export type TimeStatsDto = Schemas["TimeStatsDto"]
export type SpendStatsDto = Schemas["SpendStatsDto"]
export type ModelStatsDto = Schemas["ModelStatsDto"]
export type AttentionStatsDto = Schemas["AttentionStatsDto"]
export type ModelRank = Schemas["ModelRank"]
export type EffortDto = Schemas["EffortDto"]
export type SetModelEnabledRequest = Schemas["SetModelEnabledRequest"]
export type SetModelRankRequest = Schemas["SetModelRankRequest"]

export type AgentConfigDto = Schemas["AgentConfigDto"]

export type LogLineDto = Schemas["LogLineDto"]

export type AgentEventDto = Schemas["AgentEventDto"]
export type ResyncDto = Schemas["ResyncDto"]
export type HeartbeatDto = Schemas["HeartbeatDto"]

export type AiPermissionsStatusDto = Schemas["AiPermissionsStatusDto"]
export type PythonDto = Schemas["PythonDto"]
export type UpdateAiPermissionsRequest = Schemas["UpdateAiPermissionsRequest"]
export type TestAiPermissionRequest = Schemas["TestAiPermissionRequest"]
export type TestAiPermissionResponse = Schemas["TestAiPermissionResponse"]

export type LearnedPermissionDto = Schemas["LearnedPermissionDto"]
export type UpdateLearnedPermissionRequest = Schemas["UpdateLearnedPermissionRequest"]

/**
 * Every domain event carried by `GET /v1/events/stream`, as a tagged union.
 *
 * This stays a tagged union so the event dispatcher is exhaustive.
 */
export type DomainEvent = Schemas["DomainEvent"]
/** `"goal_updated" | "task_updated" | ...` */
export type DomainEventKind = DomainEvent["event"]

export type PullRequestDto = components["schemas"]["PullRequestDto"]
