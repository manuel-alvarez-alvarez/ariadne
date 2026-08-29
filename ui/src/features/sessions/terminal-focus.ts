/**
 * Whether the keyboard currently belongs to an agent's pane.
 *
 * Escape is a keystroke a TUI wants — Claude Code interrupts on it, Codex
 * dismisses a menu with it, and on the wire it is a plain `\x1b` — but it is
 * also how every dialog and side panel in the app is dismissed. Only one of
 * them can have it, and the one that should is whichever the user is typing
 * into: a focused pane keeps Escape, and a panel whose terminal nobody clicked
 * into closes on it exactly as it always did.
 *
 * The question is asked of the document rather than answered by the terminal.
 * The pane is several components below the panels that have to ask — the
 * session view, the terminal frame, the emulator itself — and threading a
 * "focused" flag up through all of them would give each sheet its own copy of
 * the same fact to keep in sync. `document.activeElement` already is that
 * fact, and it is still the pane's textarea while the dismissal that Escape
 * caused is being handled, which is the one moment anything reads it.
 *
 * The marker is the frame's, not xterm's: `.xterm` is a class of somebody
 * else's, and a frame that stops holding an emulator stops claiming the key.
 */

/** Put on the frame a session's pane is drawn in; see `terminal-view.tsx`. */
export const TERMINAL_FRAME_ATTRIBUTE = "data-terminal-frame"

/** Whether focus is inside a terminal pane right now. */
function terminalHasKeyboard(): boolean {
  const active = document.activeElement
  return active instanceof Element && active.closest(`[${TERMINAL_FRAME_ATTRIBUTE}]`) !== null
}

/**
 * Whether this dismissal is an Escape the pane under the pointer should have
 * been given instead — the one check every dialog holding a terminal makes
 * before it closes.
 */
export function isTerminalEscape(reason: string | undefined): boolean {
  return reason === "escape-key" && terminalHasKeyboard()
}
