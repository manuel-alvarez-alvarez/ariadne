/**
 * An id the user can take with them: the value stays the plain mono text it
 * reads as, and a small button beside it puts it on the clipboard.
 *
 * Ids in this app are 26-character ULIDs, worktree paths and commit shas —
 * things nobody retypes. They are read here and used in a terminal, so one
 * click has to be enough, and the *full* value is what gets copied even where
 * the display is shortened ({@link CopyableId.display}) or the column truncates
 * it. The text itself is no longer the button: it stays selectable, and a value
 * one may want to read is not also a target one may hit by accident. Where the
 * value is also a thing with a screen ({@link ValueProps.to}) the text is a
 * link to it, and the button beside it still only copies.
 *
 * Two shapes, because ids come in two kinds:
 *
 * - {@link CopyableId} for a value that is only ever wanted as itself — a
 *   branch, a path, a sha, a tmux name. One click, and the tooltip that names
 *   the button flips to "Copied" under the pointer: no toast, nothing that
 *   moves the layout, and it lives and dies with the value it belongs to.
 * - {@link CopyableIdMenu} for goal, task and session ids, which are usually
 *   wanted *inside* a command line (`@/lib/clipboard`). There the button
 *   opens a menu of those, and since the menu — and the thing that was clicked
 *   — is gone by the time the copy lands, the feedback is a toast naming
 *   exactly what went to the clipboard.
 *
 * When a copy fails outright (a webview with no clipboard route at all) both
 * say so instead of pretending.
 */

import { CheckIcon, CopyIcon } from "lucide-react"
import { type ReactNode, useEffect, useRef, useState } from "react"
import { Link } from "react-router-dom"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { type CopyEntry, copyText } from "@/lib/clipboard"
import { cn, middleTruncate } from "@/lib/format"

/** How long "Copied" stays up before the tooltip goes back to naming the button. */
const FEEDBACK_MS = 1400

/** Small enough to sit inside a `text-xs` row without setting its height. */
const TRIGGER_CLASS = "size-5 shrink-0 text-muted-foreground hover:text-foreground"

type ValueProps = {
  /** The full value, which is what lands on the clipboard. */
  value: string
  /** Shortens what is shown, e.g. `shortId`; the full value is still copied. */
  display?: (value: string) => string
  /**
   * Which end gets the ellipsis when the row is too narrow. `middle` keeps the
   * value's last segment on screen — for branches, whose ULID prefix is the
   * half nobody reads (see `@/lib/format`).
   */
  truncate?: "end" | "middle"
  /**
   * `name` is for a value shown as something a person reads rather than as the
   * token it is — a profile's name standing in for its id — which the mono
   * face would make look like a value to be typed.
   */
  face?: "mono" | "name"
  /** What this is, for the button and for screen readers: "task id", "branch". */
  label?: string
  /**
   * Where the value itself leads, when it is also a thing with a screen — a
   * repository path, which the Repositories screen registers. Only the text
   * becomes the link: the copy button beside it stays a button, since one
   * click has to keep meaning "give me this for my terminal".
   */
  to?: string
  /**
   * The copy button's tab stop. `-1` where the value sits on something that is
   * already *the* tab stop and is meant to stay one — a board card, which is
   * one stop by design (see `features/tasks/task-card.tsx`). It stays a
   * pointer target and keeps its name, and the keyboard route to the value is
   * the same control in the panel the card opens.
   */
  tabIndex?: number
  /**
   * The value wraps instead of being cut short, and takes a line of its own.
   * For the values that are read rather than recognised — a repository path in
   * a narrow column, where an ellipsis would hide the half that identifies it,
   * and where what follows belongs on the next line.
   */
  wrap?: boolean
  /** Sizing and colour for the row; the face of the value is {@link face}. */
  className?: string
}

/** A value with a button that copies it, and nothing else to decide. */
export function CopyableId({
  value,
  display,
  truncate,
  face,
  label = "id",
  wrap,
  to,
  tabIndex,
  className,
}: ValueProps) {
  return (
    <Value
      value={value}
      display={display}
      truncate={truncate}
      face={face}
      wrap={wrap}
      to={to}
      className={className}
    >
      <CopyButton value={value} label={label} tabIndex={tabIndex} />
    </Value>
  )
}

/** A value with a button that opens the list of things it can be copied as. */
export function CopyableIdMenu({
  value,
  display,
  truncate,
  face,
  label = "id",
  wrap,
  entries,
  className,
}: ValueProps & {
  /** What the menu offers, in order; declared by the call site. */
  entries: CopyEntry[]
}) {
  return (
    <Value
      value={value}
      display={display}
      truncate={truncate}
      face={face}
      wrap={wrap}
      className={className}
    >
      <CopyMenu label={label} entries={entries} />
    </Value>
  )
}

/**
 * The shared row: the value, then its button.
 *
 * Truncated, it sits inline — a branch or an id is one thing on a line with
 * others, and the `title` is what keeps the hidden tail readable without a
 * click, since the ellipsis is the column's doing rather than the value's.
 * Wrapped ({@link ValueProps.wrap}), the row is block-level and the whole value
 * is on screen, so it needs neither: the button stays with the first line.
 */
function Value({
  value,
  display,
  truncate = "end",
  face = "mono",
  wrap,
  to,
  className,
  children,
}: Omit<ValueProps, "label" | "tabIndex"> & { children: ReactNode }) {
  const shown = display ? display(value) : value
  const text =
    !wrap && truncate === "middle" ? (
      <MiddleTruncated value={shown} title={value} />
    ) : (
      <span
        className={cn(face === "mono" && "font-mono", wrap ? "break-all" : "truncate")}
        title={wrap ? undefined : value}
      >
        {shown}
      </span>
    )
  return (
    <span
      className={cn(
        "min-w-0 max-w-full gap-1",
        wrap ? "flex items-start" : "inline-flex items-center align-middle",
        className,
      )}
    >
      {/* The link takes the value's own box rather than adding one of its
          own, so the truncation and the wrapping above still work out of the
          row's width. */}
      {to ? (
        <Link
          to={to}
          className="flex min-w-0 rounded-xs underline-offset-3 outline-none hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
        >
          {text}
        </Link>
      ) : (
        text
      )}
      {children}
    </span>
  )
}

/**
 * The ellipsis moved into the value: two spans, the head free to shrink and
 * the tail not, so the browser eats the prefix and the name at the end stays
 * on screen. The tail may still be cut when even *it* does not fit, which is
 * the point at which nothing would have.
 */
function MiddleTruncated({ value, title }: { value: string; title: string }) {
  const { head, tail } = middleTruncate(value)
  return (
    <span className="flex min-w-0 overflow-hidden font-mono" title={title}>
      <span className="truncate">{head}</span>
      <span className="max-w-full shrink-0 truncate">{tail}</span>
    </span>
  )
}

function CopyButton({
  value,
  label,
  tabIndex,
}: {
  value: string
  label: string
  tabIndex?: number
}) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle")
  // Held open by hand, because a click on a trigger closes its own tooltip —
  // which is exactly the moment the answer has to appear. Hovering away still
  // closes it: the popup goes back to being the tooltip it was.
  const [open, setOpen] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  // The value can be swapped out under a mounted button (the panel moves to
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
    state === "copied" ? "Copied" : state === "failed" ? "Could not copy" : `Copy ${label}`

  return (
    <Tooltip open={open} onOpenChange={setOpen}>
      <TooltipTrigger
        tabIndex={tabIndex}
        render={
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Copy ${label}`}
            onClick={() => void copy()}
            className={TRIGGER_CLASS}
          />
        }
      >
        {state === "copied" ? <CheckIcon /> : <CopyIcon />}
      </TooltipTrigger>
      <TooltipContent>{hint}</TooltipContent>
    </Tooltip>
  )
}

function CopyMenu({ label, entries }: { label: string; entries: CopyEntry[] }) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Copy ${label}`}
            className={TRIGGER_CLASS}
          />
        }
      >
        <CopyIcon />
      </DropdownMenuTrigger>
      {/* Sized to its entries rather than to the button that opened it, which
          is 20 pixels wide. */}
      <DropdownMenuContent align="start" className="w-auto min-w-44">
        {entries.map((entry) => (
          <DropdownMenuItem key={entry.label} onClick={() => void copyEntry(entry)}>
            {entry.label}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** The toast quotes the text: it is the only thing left that can say what landed. */
async function copyEntry(entry: CopyEntry) {
  const ok = await copyText(entry.text)
  if (ok) toast.success("Copied", { description: entry.text })
  else toast.error("Could not copy", { description: entry.text })
}
