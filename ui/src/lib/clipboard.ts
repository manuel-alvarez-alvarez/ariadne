/**
 * What there is to copy about a goal, a task or a session, and how it gets on
 * the clipboard.
 *
 * An id is almost never wanted for its own sake: it is on its way into a
 * terminal, as the argument of one of a handful of `ariadne` commands. Those
 * command lines are spelled out here, once, so that the goal panel, the task
 * panel and both places a session id shows up offer the same entries — and so
 * that a change to the CLI's surface is a change to one file. Only commands
 * that take *this* id are listed; `ariadne attach` is the one that takes all
 * three kinds, which is why it appears in every list.
 *
 * Getting the text there has two routes. `navigator.clipboard` is the modern
 * one and the only one worth using in a browser tab, but it exists solely in a
 * secure context: the daemon is often reached over plain http on a LAN address,
 * and inside the Tauri webview the API is either missing or rejects outright. A
 * hidden textarea driven by the deprecated `execCommand("copy")` stands behind
 * it — it is what every webview still honours, as long as it runs inside the
 * click that asked for it, which is why nothing is awaited before the fallback.
 */

/** True when the text made it to the clipboard, by either route. */
export async function copyText(text: string): Promise<boolean> {
  if (typeof navigator !== "undefined" && navigator.clipboard) {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      // Denied, or no secure context. The textarea below is the second chance.
    }
  }
  return copyViaTextarea(text)
}

function copyViaTextarea(text: string): boolean {
  if (typeof document === "undefined") return false

  const textarea = document.createElement("textarea")
  textarea.value = text
  // `execCommand` copies the document's selection, so the node has to be in
  // the page and selectable — but invisible, and fixed so selecting it cannot
  // scroll whatever the user was looking at.
  textarea.setAttribute("readonly", "")
  textarea.setAttribute("aria-hidden", "true")
  textarea.style.position = "fixed"
  textarea.style.top = "0"
  textarea.style.left = "0"
  textarea.style.opacity = "0"
  document.body.append(textarea)

  try {
    textarea.select()
    return document.execCommand("copy")
  } catch {
    return false
  } finally {
    textarea.remove()
  }
}

/** One line of a copy menu: what it says, and what it puts on the clipboard. */
export type CopyEntry = {
  /** What the menu item reads, imperatively: "Copy attach command". */
  label: string
  /** Exactly what lands on the clipboard — the full id, never the shortened one. */
  text: string
}

/**
 * `ariadne attach <id>` — the one command that takes a goal, a task or a
 * session id, which is why it is spelled once here rather than in each of the
 * three lists below. The command palette offers it for whatever is open, from
 * this same spelling.
 */
export function attachCommand(id: string): string {
  return `ariadne attach ${id}`
}

export function goalCopyEntries(goalId: string): CopyEntry[] {
  return [
    { label: "Copy goal ID", text: goalId },
    { label: "Copy attach command", text: attachCommand(goalId) },
  ]
}

export function taskCopyEntries(taskId: string): CopyEntry[] {
  return [
    { label: "Copy task ID", text: taskId },
    { label: "Copy attach command", text: attachCommand(taskId) },
    { label: "Copy logs command", text: `ariadne task logs ${taskId}` },
    { label: "Copy diff command", text: `ariadne task diff ${taskId}` },
  ]
}

export function sessionCopyEntries(sessionId: string): CopyEntry[] {
  return [
    { label: "Copy session ID", text: sessionId },
    { label: "Copy attach command", text: attachCommand(sessionId) },
    { label: "Copy logs command", text: `ariadne session logs ${sessionId}` },
  ]
}
