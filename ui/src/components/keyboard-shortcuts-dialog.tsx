/**
 * Every chord the app answers to, on one sheet.
 *
 * The chords were only ever written down inside the command palette, which is
 * itself one of them: the palette says what `G S` does to somebody who already
 * knows to press ⌘K. `?` opens this instead — the convention every
 * keyboard-first app follows — and the palette lists it too, so the sheet is
 * reachable from either direction.
 *
 * The rows come from `SHORTCUT_HELP`, which the shell builds from the chords it
 * binds: what is on this sheet is what is bound, and neither can drift from the
 * other.
 */

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { SHORTCUT_HELP } from "@/hooks/use-global-shortcuts"

export function KeyboardShortcutsDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>
            The ⌘ chords answer to Ctrl as well, on every platform. The typed ones are ignored while
            a field, an editor or a session's terminal has the keystroke.
          </DialogDescription>
        </DialogHeader>
        {/* A description list, because that is what this is: the chord names
            the row and the sentence defines it. */}
        <dl className="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-2 py-2 text-sm">
          {SHORTCUT_HELP.map((shortcut) => (
            <div key={shortcut.keys} className="col-span-2 grid grid-cols-subgrid items-center">
              <dt className="justify-self-end">
                <kbd className="rounded border bg-muted px-1.5 py-0.5 font-mono text-xs whitespace-nowrap">
                  {shortcut.keys}
                </kbd>
              </dt>
              <dd className="text-muted-foreground">{shortcut.what}</dd>
            </div>
          ))}
        </dl>
      </DialogContent>
    </Dialog>
  )
}
