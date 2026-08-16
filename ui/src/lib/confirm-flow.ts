/**
 * When a confirm flow has to stay on screen, even though its own action just
 * took the reason for it away.
 *
 * An optimistic mutation puts the row into its new state on the click — and
 * that is exactly the state whose actions a screen stops offering. Left alone,
 * "Cancel goal" therefore unmounts its own confirm dialog the moment it is
 * confirmed: the spinner disappears mid-request, and a refusal rolls the cache
 * back with no dialog left to read the reason in.
 *
 * So a screen that hides its actions on a terminal status asks this first. A
 * flow is still settling while its dialog is open, while its request is in
 * flight, and while it holds an error nobody has read yet — the last of these
 * because a rollback restores the *previous* status, and the window where that
 * has landed but the dialog has not re-rendered is exactly where the error
 * would otherwise be thrown away.
 */

export interface ConfirmFlow {
  /** Its dialog is on screen. */
  open: boolean
  /** Its mutation is in flight. */
  pending: boolean
  /** Its mutation's last error, until something resets it. */
  error?: unknown
}

/** True while any of these flows still needs to be rendered. */
export function isSettling(...flows: ConfirmFlow[]): boolean {
  return flows.some((flow) => flow.open || flow.pending || flow.error != null)
}
