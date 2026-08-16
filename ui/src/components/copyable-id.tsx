/**
 * An id the user can take with them: the same mono text ids are already shown
 * in, turned into a button that puts the whole thing on the clipboard.
 *
 * Ids in this app are 26-character ULIDs, worktree paths and commit shas —
 * things nobody retypes. They are read here and used in a terminal (`ariadne
 * attach <session-id>`), so one click has to be enough, and the *full* value is
 * what gets copied even where the display is shortened ({@link display}).
 *
 * The feedback is the tooltip that told the user it was clickable in the first
 * place, flipping to "Copied" under the pointer: no toast, nothing that moves
 * the layout, and it lives and dies with the id it belongs to. When the copy
 * fails outright (a webview with no clipboard route at all) it says so instead
 * of pretending, and the text stays selectable by hand.
 */

import { useEffect, useRef, useState } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { copyText } from "@/lib/clipboard"
import { cn } from "@/lib/utils"

/** How long "Copied" stays up before the tooltip goes back to the invitation. */
const FEEDBACK_MS = 1400

export function CopyableId({
  value,
  display,
  label = "id",
  className,
}: {
  /** The full value, which is what lands on the clipboard. */
  value: string
  /** Shortens what is shown, e.g. `shortId`; the full value is still copied. */
  display?: (value: string) => string
  /** What this is, for the tooltip and for screen readers: "task id", "branch". */
  label?: string
  className?: string
}) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle")
  // Held open by hand, because a click on a trigger closes its own tooltip —
  // which is exactly the moment the answer has to appear. Hovering away still
  // closes it: the popup goes back to being the tooltip it was.
  const [open, setOpen] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  // The id can be swapped out under a mounted button (the panel moves to
  // another task), and the pending reset must not outlive it either way.
  useEffect(() => {
    return () => clearTimeout(timer.current)
  }, [])

  async function copy() {
    const ok = await copyText(value)
    setState(ok ? "copied" : "failed")
    setOpen(true)
    clearTimeout(timer.current)
    timer.current = setTimeout(() => setState("idle"), FEEDBACK_MS)
  }

  const hint =
    state === "copied" ? "Copied" : state === "failed" ? "Could not copy" : `Click to copy ${label}`

  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      <TooltipTrigger
        render={
          <button
            type="button"
            aria-label={`Copy ${label} ${value}`}
            onClick={() => void copy()}
            className={cn(
              "cursor-pointer rounded-sm text-left font-mono underline-offset-3 hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none",
              className,
            )}
          />
        }
      >
        {display ? display(value) : value}
      </TooltipTrigger>
      <TooltipContent>{hint}</TooltipContent>
    </Tooltip>
  )
}
