/**
 * What the side panels do to browser history.
 *
 * Opening one is a link (a goal on the board, a task in a lane), so it pushes:
 * the panel is somewhere the user went, and Back is how they leave it again.
 * Closing therefore steps *back* over that entry instead of writing a new one.
 * Rewriting the URL in place would look right — the panel does close — but it
 * leaves the entry that opened the panel sitting behind the closed one, and the
 * next Back reopens what was just closed. That is true of a stack as well:
 * closing the task, then the goal, unwinds the two entries that opened them.
 *
 * When there is nothing of ours behind — the panel came from a deep link or a
 * reload, so its entry is the first of the session — closing has nothing to
 * step back to and rewrites the URL instead.
 *
 * In-panel navigation (a tab, a session, a dependency) replaces, so a panel is
 * one entry however long the user stays inside it.
 */

/**
 * The params each panel owns. A closing panel takes its own state with it:
 * `tab` and `session` say where *inside* a panel the user was and mean nothing
 * once it is gone, and a goal takes the task stacked on it down with it.
 *
 * The session panel is the one `session` with no panel around it (see
 * `components/detail-panels.tsx`), so that param is all it has to take.
 */
export const PANEL_PARAMS = {
  goal: ["goal", "task", "tab", "session"],
  task: ["task", "tab", "session"],
  session: ["session"],
} as const satisfies Record<string, readonly string[]>

export type Panel = keyof typeof PANEL_PARAMS

/** Step back over the entry that opened the panel, or rewrite this one. */
export type CloseStep = { kind: "back" } | { kind: "rewrite"; search: URLSearchParams }

export function closePanel(
  panel: Panel,
  search: URLSearchParams,
  historyState: unknown,
): CloseStep {
  return canStepBack(historyState) && isTopmost(panel, search)
    ? { kind: "back" }
    : { kind: "rewrite", search: withoutPanel(panel, search) }
}

/**
 * Whether the panel being closed is the one on top of the stack.
 *
 * Only the topmost panel can be closed from the UI — the sheet under it is
 * behind the other's backdrop, and Escape goes to the top one. Closing through
 * a panel that still has one stacked on it would take the stack apart a layer
 * at a time, so it rewrites the URL and takes the whole stack down at once.
 *
 * There is only ever one stack, and only one way to stack: a task over a goal.
 * So a goal with a task on it is the single case of a panel that is not the
 * top one — the session panel opens where neither of those is, and nothing
 * opens over it.
 */
function isTopmost(panel: Panel, search: URLSearchParams): boolean {
  return panel !== "goal" || !search.has("task")
}

/**
 * Whether history holds an entry of this app's own behind the current one.
 *
 * React Router numbers the entries it creates in `history.state.idx`, from 0 at
 * whatever the session started on — so `idx > 0` is exactly "we pushed our way
 * here". Any other shape of state is not ours and counts as nothing behind,
 * which is the conservative answer: the panel closes in place.
 */
export function canStepBack(historyState: unknown): boolean {
  const idx = (historyState as { idx?: unknown } | null | undefined)?.idx
  return typeof idx === "number" && idx > 0
}

/** The same search params with the panel, and everything inside it, gone. */
export function withoutPanel(panel: Panel, search: URLSearchParams): URLSearchParams {
  const next = new URLSearchParams(search)
  for (const param of PANEL_PARAMS[panel]) next.delete(param)
  return next
}
