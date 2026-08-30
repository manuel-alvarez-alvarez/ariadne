/**
 * The app's global chords, bound once by the shell.
 *
 * One listener for all of them, on `window` and in the bubble phase: a
 * shortcut is the *last* thing a keystroke should mean, so anything that
 * handled it first — a dialog, a menu, the terminal — keeps it.
 *
 * Two vocabularies (see `@/lib/shortcuts`): ⌘ chords for the two things that
 * open over everything, and typed chords for the rest — `n` for a new goal,
 * `[` for the sidebar rail, `?` for the sheet that lists all of this, `g` then
 * a letter for the screens, the way keyboard-first apps spell navigation.
 * Typed chords are guarded twice over: never while the keystroke is
 * text (a field, an editor, a session's pane), and never from inside a dialog
 * or a menu, where a bare letter belongs to whatever is on top.
 *
 * `Escape` is deliberately absent. It belongs to whatever is on top (the
 * palette, then the panel stack), and Base UI's dialogs already close the
 * topmost one; a global handler would close two layers at a time.
 */

import { useEffect, useRef } from "react"

import {
  isTypingTarget,
  type KeySequence,
  keySequenceLabel,
  matchesHelpKey,
  matchesKeySequence,
  matchesShortcut,
  type Shortcut,
  sequenceLead,
  shortcutLabel,
  type TypingTarget,
} from "@/lib/shortcuts"
import { paths } from "@/routes/paths"

/** ⌘K / Ctrl+K — the command palette. */
export const PALETTE_SHORTCUT: Shortcut = { key: "k" }

/** ⌘, / Ctrl+, — settings, as every desktop app spells it. */
export const SETTINGS_SHORTCUT: Shortcut = { key: "," }

/** `N` — the create-goal dialog, from any screen. */
export const NEW_GOAL_SHORTCUT: KeySequence = { key: "n" }

/** `?` — the cheat sheet, which is where the rest of these are written down. */
export const HELP_SHORTCUT: KeySequence = { key: "?" }

/**
 * `[` — fold the sidebar down to an icon rail, and back.
 *
 * The bracket because every editor spells "collapse the panel on that side"
 * this way, and because it is not a letter: a screen the sidebar names has no
 * claim on it.
 */
export const SIDEBAR_SHORTCUT: KeySequence = { key: "[" }

/**
 * `G` then a letter — every screen the sidebar lists, in its order, keyed by
 * its initial (`g`oals, `s`essions, `p`rofiles, `a`gents, `r`epositories).
 *
 * Keyed by path so the palette can label its "Go to …" rows from the same list
 * the shell binds, rather than spelling the chords twice.
 */
const SCREEN_SHORTCUTS: readonly { path: string; label: string; chord: KeySequence }[] = [
  { path: paths.goals(), label: "Goals", chord: { lead: "g", key: "g" } },
  { path: paths.sessions(), label: "Sessions", chord: { lead: "g", key: "s" } },
  { path: paths.profiles(), label: "Profiles", chord: { lead: "g", key: "p" } },
  { path: paths.agents(), label: "Agents", chord: { lead: "g", key: "a" } },
  { path: paths.repositories(), label: "Repositories", chord: { lead: "g", key: "r" } },
]

/** The chord of a screen the palette lists, if it has one. */
export function screenShortcut(path: string): KeySequence | undefined {
  return SCREEN_SHORTCUTS.find((screen) => screen.path === path)?.chord
}

/** Every typed chord, which is what tells a lead key from an ordinary one. */
const TYPED_SHORTCUTS: readonly KeySequence[] = [
  NEW_GOAL_SHORTCUT,
  SIDEBAR_SHORTCUT,
  ...SCREEN_SHORTCUTS.map((screen) => screen.chord),
]

/** One line of the cheat sheet: the chord as it is typed, and what it does. */
interface ShortcutHelp {
  keys: string
  what: string
}

/**
 * Every chord there is, in the order the cheat sheet lists them.
 *
 * Spelled from the declarations above rather than written out a second time,
 * so a chord that moves cannot leave the sheet — and `ui/README.md`'s keyboard
 * table is the same list in the same order. See `ui/AGENTS.md` for how these
 * chords are bound and guarded.
 *
 * `Escape` is on it and is bound by nothing: it belongs to whatever is on top,
 * and Base UI's dialogs already close the topmost one (see the note above).
 * The sheet still has to say so — a key that works is a key worth writing
 * down, whoever implements it.
 */
export const SHORTCUT_HELP: readonly ShortcutHelp[] = [
  { keys: shortcutLabel(PALETTE_SHORTCUT), what: "Open the command palette" },
  { keys: shortcutLabel(SETTINGS_SHORTCUT), what: "Open settings" },
  { keys: keySequenceLabel(NEW_GOAL_SHORTCUT), what: "New goal, from any screen" },
  { keys: keySequenceLabel(SIDEBAR_SHORTCUT), what: "Fold the sidebar to a rail, and back" },
  ...SCREEN_SHORTCUTS.map((screen) => ({
    keys: keySequenceLabel(screen.chord),
    what: `Go to ${screen.label}`,
  })),
  { keys: keySequenceLabel(HELP_SHORTCUT), what: "Show this list" },
  { keys: "Esc", what: "Close the palette, then the topmost panel" },
]

/**
 * How long a lead key waits for its second half. Long enough to be typed
 * deliberately, short enough that a `g` typed at the wrong moment is forgotten
 * before the next unrelated keystroke means something.
 */
const SEQUENCE_TIMEOUT_MS = 1500

/** Where a bare letter is not ours: anything layered over the screen. */
const OVERLAY_SELECTOR = '[role="dialog"],[role="alertdialog"],[role="menu"],[role="listbox"]'

interface GlobalShortcutHandlers {
  onOpenPalette: () => void
  onOpenSettings: () => void
  onNewGoal: () => void
  onOpenShortcuts: () => void
  onNavigate: (path: string) => void
  onToggleSidebar: () => void
}

export function useGlobalShortcuts({
  onOpenPalette,
  onOpenSettings,
  onNewGoal,
  onOpenShortcuts,
  onNavigate,
  onToggleSidebar,
}: GlobalShortcutHandlers): void {
  /** The lead key waiting for the rest of its sequence. */
  const pending = useRef<{ key: string; timer: number } | null>(null)

  useEffect(() => {
    function clearPending() {
      if (pending.current) window.clearTimeout(pending.current.timer)
      pending.current = null
    }

    function onKeyDown(event: KeyboardEvent) {
      // Somebody already acted on this keystroke (an open combobox, xterm's
      // own bindings); a second meaning would be a surprise.
      if (event.defaultPrevented) return
      // Where the keystroke is going owns it: a text field, an editor, or the
      // pane a session is being typed into.
      if (isTypingTarget(event.target as TypingTarget | null)) return

      const held = matchesShortcut(event, PALETTE_SHORTCUT)
        ? onOpenPalette
        : matchesShortcut(event, SETTINGS_SHORTCUT)
          ? onOpenSettings
          : null
      if (held) {
        clearPending()
        // Both chords are the browser's too (⌘K focuses the address bar in
        // Chrome, ⌘, opens preferences in Safari) — claim them.
        event.preventDefault()
        held()
        return
      }

      // A dialog or a menu is up: a bare letter is being typed *at it*.
      if (event.target instanceof Element && event.target.closest(OVERLAY_SELECTOR)) return

      const lead = pending.current?.key ?? null
      const screen = SCREEN_SHORTCUTS.find(({ chord }) => matchesKeySequence(event, chord, lead))
      const handler = screen
        ? () => onNavigate(screen.path)
        : matchesKeySequence(event, NEW_GOAL_SHORTCUT, lead)
          ? onNewGoal
          : matchesKeySequence(event, SIDEBAR_SHORTCUT, lead)
            ? onToggleSidebar
            : // Typed, but not bare: `?` carries the Shift it takes to type it.
              matchesHelpKey(event)
              ? onOpenShortcuts
              : null

      if (handler) {
        clearPending()
        event.preventDefault()
        handler()
        return
      }

      // Not a chord: either it opens one, or it ends the wait for the lead that
      // was pending — a `g` followed by anything else is nothing at all.
      const opens = sequenceLead(event, TYPED_SHORTCUTS)
      clearPending()
      if (opens) {
        event.preventDefault()
        pending.current = {
          key: opens,
          timer: window.setTimeout(() => {
            pending.current = null
          }, SEQUENCE_TIMEOUT_MS),
        }
      }
    }

    window.addEventListener("keydown", onKeyDown)
    return () => {
      window.removeEventListener("keydown", onKeyDown)
      clearPending()
    }
  }, [onOpenPalette, onOpenSettings, onNewGoal, onOpenShortcuts, onNavigate, onToggleSidebar])
}
