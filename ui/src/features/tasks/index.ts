/**
 * What the rest of the app may use from this feature.
 *
 * The goals board is built from the task row, the status model and the task
 * list query exported here, and the goal panel lists the card. The task side
 * panel is mounted by the shell's `DetailPanels`.
 */

export { taskListQueryOptions } from "./queries"
export { STALLED_META, StalledBadge } from "./stalled"
export { compareByAttention, TASK_STATUS_META } from "./status"
export { TaskCard } from "./task-card"
export { TaskPanel } from "./task-panel"
export { TaskRow } from "./task-row"
