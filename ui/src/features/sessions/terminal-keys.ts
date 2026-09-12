/**
 * A DOM key event as the terminal socket's `key` message: the key code and
 * the modifiers crossterm would have read from a terminal (008).
 *
 * The console reads keys, not characters, so the mapping is the browser's
 * `key` onto crossterm's `KeyCode`: a printable character as itself, the
 * named keys by name, and F1 to F12 by number. Two things a terminal never
 * shows the console are kept from it here too. A chord held with the command
 * key on macOS is the app's or the browser's — ⌘C copies the selection, ⌘V
 * raises the paste event the pane sends as a `paste` — and Ctrl+Shift+C and
 * Ctrl+Shift+V are the same pair everywhere else. And Option on macOS turns
 * a letter into a symbol (Option-B is ∫), where the console wants Alt-B for
 * a word left: the letter is read off the physical key instead.
 */

import type { TerminalClientMessage, TerminalKey, TerminalModifier } from "./terminal-socket"

/** What the mapping reads of a `KeyboardEvent`. */
interface KeyInput {
  key: string
  code: string
  shiftKey: boolean
  ctrlKey: boolean
  altKey: boolean
  metaKey: boolean
}

const NAMED_KEYS: Record<string, TerminalKey> = {
  Enter: "enter",
  Backspace: "backspace",
  Tab: "tab",
  Escape: "esc",
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  Home: "home",
  End: "end",
  PageUp: "page_up",
  PageDown: "page_down",
  Delete: "delete",
  Insert: "insert",
}

/**
 * The `key` message for a key press, or `null` for a press the console is
 * not sent: a bare modifier, a command-key chord, or the copy and paste
 * chords the browser answers itself.
 */
export function terminalKeyMessage(event: KeyInput): TerminalClientMessage | null {
  if (event.metaKey) return null
  if (event.ctrlKey && event.shiftKey && /^[cv]$/i.test(event.key)) return null

  const code = keyCode(event)
  if (code === null) return null

  const modifiers: TerminalModifier[] = []
  if (event.shiftKey) modifiers.push("shift")
  if (event.ctrlKey) modifiers.push("control")
  if (event.altKey) modifiers.push("alt")
  return { type: "key", code, modifiers }
}

function keyCode(event: KeyInput): TerminalKey | null {
  if (event.key === "Tab" && event.shiftKey) return "back_tab"
  const named = NAMED_KEYS[event.key]
  if (named !== undefined) return named

  const fn = /^F(\d{1,2})$/.exec(event.key)
  if (fn?.[1] !== undefined) return { f: Number(fn[1]) }

  const letter = /^Key([A-Z])$/.exec(event.code)?.[1]
  if (event.altKey && letter !== undefined) {
    return { char: event.shiftKey ? letter : letter.toLowerCase() }
  }
  // One code point, which is what a printable key produces — and what a
  // named key like "Shift" or "Dead" does not.
  if ([...event.key].length === 1) return { char: event.key }
  return null
}
