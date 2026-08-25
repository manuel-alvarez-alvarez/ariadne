/**
 * What counts as a keyboard shortcut, and when one is allowed to fire.
 *
 * App-wide, like `@/lib/format` and `@/lib/format`: the shell binds the shortcuts
 * (see `@/hooks/use-global-shortcuts`), but the two questions this file answers
 * — "is this the chord?" and "is the user typing?" — are pure and are what the
 * tests pin.
 *
 * **The modifier is either one.** ⌘ on macOS, Ctrl everywhere else is the
 * convention, but the app runs in a Tauri WebView, in a browser tab, and on
 * three platforms, and a chord that silently does nothing because the platform
 * was sniffed wrong is worse than one that answers to both. Only the *hint*
 * printed next to the affordance picks a side.
 *
 * **Two kinds of chord.** The ones held with the modifier ({@link Shortcut}),
 * and the ones *typed* ({@link KeySequence}): a bare `n`, or `g` then a letter
 * for the screens. Typed chords are only safe because of the guard below — a
 * bare key means "go to sessions" exactly where it does not mean the letter s.
 */

/** A chord: one key, with the platform's command modifier held. */
export interface Shortcut {
  /** `KeyboardEvent.key`, matched case-insensitively. */
  key: string
}

/**
 * A chord that is typed rather than held: a bare key, or two of them in a row —
 * `g` `s`, the way every keyboard-first app spells "go to sessions".
 */
export interface KeySequence {
  /** The key that opens the sequence. Absent for a chord that is one key. */
  lead?: string
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

/** Whether nothing is held down — what makes a keystroke a *typed* chord. */
export function isBareKey(event: ShortcutEvent): boolean {
  return !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
}

/**
 * Whether the event completes this sequence, given the lead key waiting for its
 * second half — `null` when none is.
 *
 * A pending lead is part of the match rather than a separate check: with `g`
 * pending, a lone `n` is the tail of a sequence nobody bound, not "new goal".
 */
export function matchesKeySequence(
  event: ShortcutEvent,
  sequence: KeySequence,
  pending: string | null,
): boolean {
  if (!isBareKey(event)) return false
  if ((sequence.lead ?? null) !== pending) return false
  return event.key.toLowerCase() === sequence.key.toLowerCase()
}

/**
 * The lead this event opens — the `g` of `g s` — or `null` when it opens none
 * of these sequences. What the shell holds until the next keystroke.
 */
export function sequenceLead(
  event: ShortcutEvent,
  sequences: readonly KeySequence[],
): string | null {
  if (!isBareKey(event)) return null
  const key = event.key.toLowerCase()
  return sequences.some((sequence) => sequence.lead?.toLowerCase() === key) ? key : null
}

/** A typed chord as it should be shown: `N`, or `G S`. */
export function keySequenceLabel(sequence: KeySequence): string {
  const key = sequence.key.toUpperCase()
  return sequence.lead ? `${sequence.lead.toUpperCase()} ${key}` : key
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
function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false
  return /mac|iphone|ipad|ipod/i.test(navigator.platform || navigator.userAgent)
}

/** A chord as it should be shown, e.g. `⌘K` or `Ctrl+K`. */
export function shortcutLabel(shortcut: Shortcut): string {
  const key = shortcut.key.length === 1 ? shortcut.key.toUpperCase() : shortcut.key
  return isApplePlatform() ? `⌘${key}` : `Ctrl+${key}`
}
