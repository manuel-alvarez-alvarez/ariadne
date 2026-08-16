/**
 * What there is to copy about a goal, a task or a session.
 *
 * An id is almost never wanted for its own sake: it is on its way into a
 * terminal, as the argument of one of a handful of `ariadne` commands. Those
 * command lines are spelled out here, once, so that the goal panel, the task
 * panel and both places a session id shows up offer the same entries — and so
 * that a change to the CLI's surface is a change to one file.
 *
 * Only commands that take *this* id are listed. `ariadne attach` is the one
 * that takes all three kinds, which is why it appears in every list.
 */

/** One line of a copy menu: what it says, and what it puts on the clipboard. */
export type CopyEntry = {
  /** What the menu item reads, imperatively: "Copy attach command". */
  label: string
  /** Exactly what lands on the clipboard — the full id, never the shortened one. */
  text: string
}

export function goalCopyEntries(goalId: string): CopyEntry[] {
  return [
    { label: "Copy goal ID", text: goalId },
    { label: "Copy attach command", text: `ariadne attach ${goalId}` },
  ]
}

export function taskCopyEntries(taskId: string): CopyEntry[] {
  return [
    { label: "Copy task ID", text: taskId },
    { label: "Copy attach command", text: `ariadne attach ${taskId}` },
    { label: "Copy logs command", text: `ariadne task logs ${taskId}` },
    { label: "Copy diff command", text: `ariadne task diff ${taskId}` },
  ]
}

export function sessionCopyEntries(sessionId: string): CopyEntry[] {
  return [
    { label: "Copy session ID", text: sessionId },
    { label: "Copy attach command", text: `ariadne attach ${sessionId}` },
    { label: "Copy logs command", text: `ariadne session logs ${sessionId}` },
  ]
}
