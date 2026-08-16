/**
 * What the rest of the app may use from this feature.
 *
 * The board is mounted by the goal detail screen, which the goals feature owns
 * — everything else here is reached through `taskRoutes`.
 */

export { tasksPath } from "./paths"
export { TaskBoard } from "./task-board"
