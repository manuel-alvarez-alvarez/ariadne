/**
 * The app's global chords, bound once by the shell.
 *
 * One listener for all of them, on `window` and in the bubble phase: a
 * shortcut is the *last* thing a keystroke should mean, so anything that
 * handled it first — a dialog, a menu, the terminal — keeps it.
 *
 * Two vocabularies (see `@/lib/shortcuts`): ⌘ chords for the two things that
 * open over everything, and typed chords for the rest — `n` for a new goal,
 * `g` then a letter for the four screens, the way keyboard-first apps spell
 * navigation. Typed chords are guarded twice over: never while the keystroke is
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
  matchesKeySequence,
  matchesShortcut,
  type Shortcut,
  sequenceLead,
  type TypingTarget,
} from "@/lib/shortcuts"
import { paths } from "@/routes/paths"

/** ⌘K / Ctrl+K — the command palette. */
export const PALETTE_SHORTCUT: Shortcut = { key: "k" }

/** ⌘, / Ctrl+, — settings, as every desktop app spells it. */
export const SETTINGS_SHORTCUT: Shortcut = { key: "," }

/** `N` — the create-goal dialog, from any screen. */
export const NEW_GOAL_SHORTCUT: KeySequence = { key: "n" }

/**
 * `G` then a letter — the four screens, in the sidebar's order, keyed by their
 * initial where that is free (`g`oals, `a`ttention, `s`essions, `p`rofiles).
 *
 * Keyed by path so the palette can label its "Go to …" rows from the same list
 * the shell binds, rather than spelling the chords twice.
 */
export const SCREEN_SHORTCUTS: readonly { path: string; chord: KeySequence }[] = [
  { path: paths.goals(), chord: { lead: "g", key: "g" } },
  { path: paths.attention(), chord: { lead: "g", key: "a" } },
  { path: paths.sessions(), chord: { lead: "g", key: "s" } },
  { path: paths.profiles(), chord: { lead: "g", key: "p" } },
]

/** The chord of a screen the palette lists, if it has one. */
export function screenShortcut(path: string): KeySequence | undefined {
  return SCREEN_SHORTCUTS.find((screen) => screen.path === path)?.chord
}

/** Every typed chord, which is what tells a lead key from an ordinary one. */
const TYPED_SHORTCUTS: readonly KeySequence[] = [
  NEW_GOAL_SHORTCUT,
  ...SCREEN_SHORTCUTS.map((screen) => screen.chord),
]

/**
 * How long a lead key waits for its second half. Long enough to be typed
 * deliberately, short enough that a `g` typed at the wrong moment is forgotten
 * before the next unrelated keystroke means something.
 */
const SEQUENCE_TIMEOUT_MS = 1500

/** Where a bare letter is not ours: anything layered over the screen. */
const OVERLAY_SELECTOR = '[role="dialog"],[role="alertdialog"],[role="menu"],[role="listbox"]'

export interface GlobalShortcutHandlers {
  onOpenPalette: () => void
  onOpenSettings: () => void
  onNewGoal: () => void
  onNavigate: (path: string) => void
}

export function useGlobalShortcuts({
  onOpenPalette,
  onOpenSettings,
  onNewGoal,
  onNavigate,
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
  }, [onOpenPalette, onOpenSettings, onNewGoal, onNavigate])
}
