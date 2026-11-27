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

import { ChildStepsContext, type FullView, FullViewContext } from "./full-view-context"
import { StepBody } from "./step-detail"

/** The narrowest console panel that shows the full view beside the conversation. */
const WIDE_PX = 880

export function StepWorkspace({
  host,
  sessionId,
  className,
  childSteps,
  children,
}: {
  host: Host
  sessionId: string
  /** The live steps of the subagents' lanes, by step id. */
  childSteps: Readonly<Record<string, StepDto>>
  /** The classes of the conversation's own box. */
  className?: string
  children: ReactNode
}) {
  const root = useRef<HTMLDivElement>(null)
  const aside = useRef<HTMLElement>(null)
  const bodyBox = useRef<HTMLDivElement>(null)
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

  const shownId = useRef<string | undefined>(undefined)
  shownId.current = step?.id

  /**
   * The control the keyboard goes back to: the one that opened the view, else
   * the row of the step shown, else the console's own box.
   */
  const returnTarget = useCallback((): HTMLElement | null => {
    const back = trigger.current
    trigger.current = null
    if (back?.isConnected) return back
    const id = shownId.current
    const row = id
      ? root.current?.querySelector<HTMLElement>(`button[data-step-id="${CSS.escape(id)}"]`)
      : null
    return row ?? root.current
  }, [])

  // The drawer gives the keyboard back itself, through its `finalFocus`.
  const dismiss = useCallback(() => {
    const target = wide ? returnTarget() : null
    setStep(undefined)
    target?.focus()
  }, [wide, returnTarget])

  const view = useMemo<FullView>(
    () => ({
      shownId: step?.id,
      show: (next, from) => {
        // An Open inside the view keeps the control that opened the view at first.
        if (!from || !bodyBox.current?.contains(from)) trigger.current = from ?? null
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
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.metaKey) return
      event.preventDefault()
      event.stopPropagation()
      dismiss()
    }
    element.addEventListener("keydown", onKey)
    return () => element.removeEventListener("keydown", onKey)
  }, [shown, wide, dismiss])

  // Each step the view shows hands the keyboard to its Close button: an Open
  // inside the view unmounts with the body it was in.
  const id = step?.id
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new step or a new placement moves the keyboard
  useEffect(() => {
    if (id !== undefined) close.current?.focus()
  }, [id, wide])

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
    <div ref={bodyBox} className="min-h-0 flex-1 overflow-y-auto p-3">
      {/* A new step is a new body: nothing the last one showed, its Raw included, stays. */}
      <StepBody
        key={step.id}
        host={host}
        sessionId={sessionId}
        step={step}
        now={new Date().toISOString()}
        full
      />
    </div>
  ) : null

  return (
    <FullViewContext.Provider value={view}>
      <ChildStepsContext.Provider value={childSteps}>
        <div
          ref={root}
          data-step-workspace
          tabIndex={-1}
          className="flex min-h-0 flex-1 outline-none"
        >
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
              finalFocus={() => returnTarget() ?? false}
              onKeyDown={(event) => {
                if (event.key === "Escape") event.stopPropagation()
              }}
            >
              {header}
              {body}
            </SheetContent>
          </Sheet>
        )}
      </ChildStepsContext.Provider>
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
