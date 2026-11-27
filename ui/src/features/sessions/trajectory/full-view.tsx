/**
 * The full view of one step: its whole content, with no set height. On a
 * console panel of 880 CSS pixels or wider it sits beside the conversation,
 * and on a narrower panel it is a modal drawer. The width is the panel's,
 * not the browser's, as the console of today measures it (`workspace.tsx`).
 *
 * Escape in the full view closes it, and the keyboard goes back to the
 * control that opened it. The Escape stops at the view, so it never reaches
 * the console, which would cancel the running turn.
 */

import { XIcon } from "lucide-react"
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react"

import type { Host, StepDto } from "@/api"
import { Button } from "@/components/ui/button"
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet"
import { cn } from "@/lib/format"

import { type FullView, FullViewContext } from "./full-view-context"
import { StepBody } from "./step-detail"

/** The narrowest console panel that shows the full view beside the conversation. */
const WIDE_PX = 880

export function StepWorkspace({
  host,
  sessionId,
  className,
  children,
}: {
  host: Host
  sessionId: string
  /** The classes of the conversation's own box. */
  className?: string
  children: ReactNode
}) {
  const root = useRef<HTMLDivElement>(null)
  const aside = useRef<HTMLElement>(null)
  const close = useRef<HTMLButtonElement>(null)
  const trigger = useRef<HTMLElement | null>(null)
  const [step, setStep] = useState<StepDto | undefined>(undefined)
  const [wide, setWide] = useState(false)

  useEffect(() => {
    const element = root.current
    if (!element) return
    setWide(element.getBoundingClientRect().width >= WIDE_PX)
    if (typeof ResizeObserver === "undefined") return
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWide(entry.contentRect.width >= WIDE_PX)
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  const dismiss = useCallback(() => {
    setStep(undefined)
    const back = trigger.current
    trigger.current = null
    if (back?.isConnected) back.focus()
  }, [])

  const view = useMemo<FullView>(
    () => ({
      shownId: step?.id,
      show: (next, from) => {
        trigger.current = from ?? null
        setStep(next)
      },
      refresh: (next) => setStep((shown) => (shown?.id === next.id ? next : shown)),
    }),
    [step?.id],
  )

  // The aside is inside the console, whose own Escape cancels the turn: the
  // view claims the key first, on its own element.
  const shown = step !== undefined
  useEffect(() => {
    const element = aside.current
    if (!shown || !wide || !element) return
    close.current?.focus()
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.metaKey) return
      event.preventDefault()
      event.stopPropagation()
      dismiss()
    }
    element.addEventListener("keydown", onKey)
    return () => element.removeEventListener("keydown", onKey)
  }, [shown, wide, dismiss])

  const title = step ? viewTitle(step) : "Step"
  const header = (
    <div className="flex items-center justify-between gap-2 border-b px-3 py-1.5">
      {wide ? (
        <h3 className="min-w-0 truncate text-sm font-medium">{title}</h3>
      ) : (
        <SheetTitle className="min-w-0 truncate text-sm">{title}</SheetTitle>
      )}
      <Button
        ref={close}
        size="icon-xs"
        variant="ghost"
        onClick={dismiss}
        aria-label="Close the full view"
      >
        <XIcon />
      </Button>
    </div>
  )
  const body = step ? (
    <div className="min-h-0 flex-1 overflow-y-auto p-3">
      <StepBody host={host} sessionId={sessionId} step={step} now={new Date().toISOString()} full />
    </div>
  ) : null

  return (
    <FullViewContext.Provider value={view}>
      <div ref={root} data-step-workspace className="flex min-h-0 flex-1">
        <div className={cn("min-w-0", className)}>{children}</div>
        {wide && step ? (
          <aside
            ref={aside}
            aria-label="Full view"
            className="flex min-h-0 w-[45%] min-w-0 flex-col border-l bg-background"
          >
            {header}
            {body}
          </aside>
        ) : null}
      </div>
      {wide ? null : (
        <Sheet
          open={step !== undefined}
          onOpenChange={(open) => {
            if (!open) setStep(undefined)
          }}
        >
          <SheetContent
            showCloseButton={false}
            className="flex w-full flex-col gap-0 p-0 sm:max-w-2xl"
            initialFocus={() => close.current}
            finalFocus={() => {
              const back = trigger.current
              trigger.current = null
              return back?.isConnected ? back : false
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") event.stopPropagation()
            }}
          >
            {header}
            {body}
          </SheetContent>
        </Sheet>
      )}
    </FullViewContext.Provider>
  )
}

function viewTitle(step: StepDto): string {
  switch (step.type) {
    case "tool_call":
    case "subagent":
      return step.title
    case "permission":
      return step.tool.title
    default:
      return step.type
  }
}
