/**
 * What the rest of the app may use from this feature.
 *
 * The goals board's swimlanes are built from the card, the status model and
 * the task list query exported here; the task side panel is mounted by the
 * shell's `DetailPanels`.
 */

export { taskListQueryOptions } from "./queries"
export { BOARD_STATUSES, OFF_BOARD_STATUSES, primaryStatus, TASK_STATUS_META } from "./status"
export { TaskCard } from "./task-card"
export { TaskPanel } from "./task-panel"
