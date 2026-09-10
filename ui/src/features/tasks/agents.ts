/**
 * A task's agents, by the seat they sit in.
 *
 * The DTO carries them as one list, author first and the reviewers in review
 * order, because that is the order they are staffed in and the order they are
 * read in. Every screen that shows one side or the other asks here rather than
 * filtering inline, so "the author" means the same thing on all of them.
 */

import type { TaskAgentDto, TaskDto } from "@/api"

/**
 * The one agent that authors the task, or undefined for a task whose staffing
 * is gone — which the daemon does not allow, and which a screen still has to
 * render rather than throw on.
 *
 * The first author on a task staffed with several — screens that only know
 * one side of a multi-author task (the edit form, outside-session matching)
 * read this one, same as before several authors existed.
 */
export function taskAuthor(task: { agents: TaskAgentDto[] }): TaskAgentDto | undefined {
  return task.agents.find((agent) => agent.seat === "author")
}

/** Every author on the task, in staffing order. Most tasks have one. */
export function taskAuthors(task: { agents: TaskAgentDto[] }): TaskAgentDto[] {
  return task.agents.filter((agent) => agent.seat === "author")
}

/** The reviewers, in review order. */
export function taskReviewers(task: { agents: TaskAgentDto[] }): TaskAgentDto[] {
  return task.agents.filter((agent) => agent.seat === "reviewer")
}

export type { TaskDto }
