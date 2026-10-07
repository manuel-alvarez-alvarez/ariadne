/**
 * What the side panels do to browser history.
 *
 * Opening one from a list is a link (a goal on the board, a task in a lane
 * with nothing else open), so it pushes: the panel is somewhere the user
 * went, and Back is how they leave it again. Closing therefore steps *back*
 * over that entry instead of writing a new one. Rewriting the URL in place
 * would look right — the panel does close — but it leaves the entry that
 * opened the panel sitting behind the closed one, and the next Back reopens
 * what was just closed.
 *
 * When there is nothing of ours behind — the panel came from a deep link or a
 * reload, so its entry is the first of the session — closing has nothing to
 * step back to and rewrites the URL instead.
 *
 * In-panel navigation (a tab, a session, a dependency, a task opened over a
 * goal) replaces rather than pushes, so a panel is one entry however long the
 * user stays inside it and whatever else it shows along the way — see
 * `features/tasks/task-card.tsx` and `task-panel.tsx`'s breadcrumb, which is
 * why a task and a goal never share a history entry's params for `closePanel`
 * to have to tell apart.
 */

/**
 * The params each panel owns. A closing panel takes its own state with it:
 * `tab` and `session` say where *inside* a panel the user was and mean nothing
 * once it is gone. A task also drops a stray `goal` — the two never open
 * together on purpose, but a hand-written URL can still hold both, and a task
 * replaces the goal rather than sitting over it.
 *
 * The session panel is the one `session` with no panel around it (see
 * `components/detail-panels.tsx`); its `tab` is the session view's own
 * terminal/activity strip, which is inside it like any other.
 */
const PANEL_PARAMS = {
  pr: ["pr"],
  goal: ["goal", "task", "tab", "session"],
  task: ["task", "goal", "tab", "session"],
  session: ["session", "tab"],
} as const satisfies Record<string, readonly string[]>

export type Panel = keyof typeof PANEL_PARAMS

/** Step back over the entry that opened the panel, or rewrite this one. */
type CloseStep = { kind: "back" } | { kind: "rewrite"; search: URLSearchParams }

export function closePanel(
  panel: Panel,
  search: URLSearchParams,
  historyState: unknown,
): CloseStep {
  return canStepBack(historyState)
    ? { kind: "back" }
    : { kind: "rewrite", search: withoutPanel(panel, search) }
}

/**
 * Whether history holds an entry of this app's own behind the current one.
 *
 * React Router numbers the entries it creates in `history.state.idx`, from 0 at
 * whatever the session started on — so `idx > 0` is exactly "we pushed our way
 * here". Any other shape of state is not ours and counts as nothing behind,
 * which is the conservative answer: the panel closes in place.
 */
function canStepBack(historyState: unknown): boolean {
  const idx = (historyState as { idx?: unknown } | null | undefined)?.idx
  return typeof idx === "number" && idx > 0
}

/** The same search params with the panel, and everything inside it, gone. */
function withoutPanel(panel: Panel, search: URLSearchParams): URLSearchParams {
  const next = new URLSearchParams(search)
  for (const param of PANEL_PARAMS[panel]) next.delete(param)
  return next
}
