/**
 * The app's global chords, bound once by the shell.
 *
 * One listener for all of them, on `window` and in the bubble phase: a
 * shortcut is the *last* thing a keystroke should mean, so anything that
 * handled it first — a dialog, a menu, the terminal — keeps it.
 *
 * `Escape` is deliberately absent. It belongs to whatever is on top (the
 * palette, then the panel stack), and Base UI's dialogs already close the
 * topmost one; a global handler would close two layers at a time.
 */

import { useEffect } from "react"

import { isTypingTarget, matchesShortcut, type Shortcut, type TypingTarget } from "@/lib/shortcuts"

/** ⌘K / Ctrl+K — the command palette. */
export const PALETTE_SHORTCUT: Shortcut = { key: "k" }

/** ⌘, / Ctrl+, — settings, as every desktop app spells it. */
export const SETTINGS_SHORTCUT: Shortcut = { key: "," }

export interface GlobalShortcutHandlers {
  onOpenPalette: () => void
  onOpenSettings: () => void
}

export function useGlobalShortcuts({
  onOpenPalette,
  onOpenSettings,
}: GlobalShortcutHandlers): void {
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      // Somebody already acted on this keystroke (an open combobox, xterm's
      // own bindings); a second meaning would be a surprise.
      if (event.defaultPrevented) return
      // Where the keystroke is going owns it: a text field, an editor, or the
      // pane a session is being typed into.
      if (isTypingTarget(event.target as TypingTarget | null)) return

      const handler = matchesShortcut(event, PALETTE_SHORTCUT)
        ? onOpenPalette
        : matchesShortcut(event, SETTINGS_SHORTCUT)
          ? onOpenSettings
          : null
      if (!handler) return

      // Both chords are the browser's too (⌘K focuses the address bar in
      // Chrome, ⌘, opens preferences in Safari) — claim them.
      event.preventDefault()
      handler()
    }

    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [onOpenPalette, onOpenSettings])
}
