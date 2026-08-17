/**
 * What counts as a keyboard shortcut, and when one is allowed to fire.
 *
 * App-wide, like `@/lib/ids` and `@/lib/time`: the shell binds the shortcuts
 * (see `@/hooks/use-global-shortcuts`), but the two questions this file answers
 * — "is this the chord?" and "is the user typing?" — are pure and are what the
 * tests pin.
 *
 * **The modifier is either one.** ⌘ on macOS, Ctrl everywhere else is the
 * convention, but the app runs in a Tauri WebView, in a browser tab, and on
 * three platforms, and a chord that silently does nothing because the platform
 * was sniffed wrong is worse than one that answers to both. Only the *hint*
 * printed next to the affordance picks a side.
 */

/** A chord: one key, with the platform's command modifier held. */
export interface Shortcut {
  /** `KeyboardEvent.key`, matched case-insensitively. */
  key: string
}

/** The bit of a keyboard event this module needs; keeps the tests DOM-free. */
export interface ShortcutEvent {
  key: string
  metaKey: boolean
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
}

/**
 * Whether the event is that chord.
 *
 * Alt and Shift must be up: `⌥⌘K` and `⇧⌘K` are other applications' chords
 * (and, in the WebView, other characters), not sloppier spellings of this one.
 */
export function matchesShortcut(event: ShortcutEvent, shortcut: Shortcut): boolean {
  return (
    (event.metaKey || event.ctrlKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === shortcut.key.toLowerCase()
  )
}

/** The bit of the focused element the guard reads. Keeps the check DOM-free. */
export interface TypingTarget {
  tagName?: string
  /** Set by the DOM on every node *inside* a `contenteditable`, not just on it. */
  isContentEditable?: boolean
}

/** Elements that are text entry by nature, whatever is focused inside them. */
const TYPING_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"])

/**
 * Whether the keystroke is going somewhere that owns its keyboard: a text
 * field, a `contenteditable` (CodeMirror's editors), or the hidden textarea
 * xterm reads a session's pane through — where ⌘K belongs to the pane, not to
 * us.
 */
export function isTypingTarget(target: TypingTarget | null | undefined): boolean {
  if (!target) return false
  if (target.isContentEditable) return true
  return typeof target.tagName === "string" && TYPING_TAGS.has(target.tagName.toUpperCase())
}

/**
 * Whether to print ⌘ or Ctrl in the hints. A guess by design — it decides how
 * a chord is *spelled*, never whether it fires — so the old-but-universal
 * `platform` sniff is enough, and a wrong answer costs a wrong glyph.
 */
export function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false
  return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent)
}

/** The command modifier as it should be shown on this platform: `⌘` or `Ctrl`. */
export function modifierLabel(): string {
  return isApplePlatform() ? "⌘" : "Ctrl"
}

/** A chord as it should be shown, e.g. `⌘K` or `Ctrl+K`. */
export function shortcutLabel(shortcut: Shortcut): string {
  const key = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : shortcut.key
  return isApplePlatform() ? `⌘${key}` : `Ctrl+${key}`
}
