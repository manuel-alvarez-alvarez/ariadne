/**
 * The inspector's frame: it floats over the screen at the window's right edge,
 * above a scrim, so the screen keeps its full width and layout behind it. A
 * click on the scrim closes it, and Tab stays inside it while it is open.
 * Below `md` it covers the window at full width.
 */

import { XIcon } from "lucide-react"
import {
  type ComponentProps,
  type CSSProperties,
  createContext,
  type KeyboardEvent,
  useContext,
  useEffect,
  useId,
  useRef,
  useState,
} from "react"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/format"
import { useSettingsStore } from "@/stores/settings"

const PaneTitleId = createContext<string | undefined>(undefined)

/** What Tab can land on: the first one and the last one are where it wraps. */
const FOCUSABLE =
  'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])'

/** Tab from the last stop goes to the first, and Shift+Tab from the first to the last. */
function trapTab(event: KeyboardEvent<HTMLDivElement>) {
  const pane = event.currentTarget
  if (event.key !== "Tab" || !pane.contains(event.target as Node)) return
  // The resize handle is hidden below `md`, and a hidden stop is no stop.
  const stops = [...pane.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (stop) => stop.checkVisibility?.() ?? true,
  )
  const first = stops[0]
  const last = stops.at(-1)
  if (!first || !last) return
  const wrap = event.shiftKey
    ? (event.target === first || event.target === pane) && last
    : event.target === last && first
  if (!wrap) return
  event.preventDefault()
  wrap.focus()
}

export function DockedPane({
  onClose,
  onKeyDown,
  onFocusCapture,
  children,
  ...props
}: ComponentProps<"div"> & { onClose: () => void }) {
  const titleId = useId()
  const focused = useRef<{ pane: HTMLDivElement; control: Element } | null>(null)

  useEffect(() => {
    // React focus events include this pane's modal portals. Observe removals
    // there too, since a dialog can close after its opener has disappeared.
    const observer = new MutationObserver(() => {
      const last = focused.current
      if (
        last?.pane.isConnected &&
        !last.control.isConnected &&
        document.activeElement === document.body
      ) {
        last.pane.focus()
      }
    })
    const forgetOutsideFocus = (event: FocusEvent) => {
      if (event.target !== focused.current?.control) focused.current = null
    }
    observer.observe(document.body, { childList: true, subtree: true })
    document.addEventListener("focusin", forgetOutsideFocus)
    return () => {
      observer.disconnect()
      document.removeEventListener("focusin", forgetOutsideFocus)
    }
  }, [])
  const savedWidth = useSettingsStore((state) => state.panelWidth)
  const setWidth = useSettingsStore((state) => state.setPanelWidth)
  const [viewport, setViewport] = useState(window.innerWidth)
  // A drag moves this width only; the store takes it once, on release.
  const [dragWidth, setDragWidth] = useState<number | null>(null)
  const drag = useRef<{ pointer: number; x: number; from: number; to: number | null } | null>(null)
  const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
  const minimum = 24 * rem
  const maximum = Math.max(minimum, viewport * 0.6)
  const clamp = (value: number) => Math.min(maximum, Math.max(minimum, value))
  const width = clamp(dragWidth ?? savedWidth)

  useEffect(() => {
    const resize = () => setViewport(window.innerWidth)
    window.addEventListener("resize", resize)
    return () => {
      window.removeEventListener("resize", resize)
      // A close in mid-drag, by Escape say, still keeps the width dragged to.
      const to = drag.current?.to
      if (to != null) useSettingsStore.getState().setPanelWidth(to)
    }
  }, [])

  const endDrag = () => {
    const to = drag.current?.to
    drag.current = null
    if (to != null) setWidth(to)
    setDragWidth(null)
  }

  return (
    <PaneTitleId.Provider value={titleId}>
      {/* A pointer's way out; Escape and the close button are the keyboard's. */}
      <div
        data-slot="docked-pane-scrim"
        aria-hidden="true"
        className="fixed inset-0 z-40 bg-black/20 duration-150 animate-in fade-in-0"
        onClick={onClose}
      />
      <div
        data-slot="docked-pane"
        role="region"
        aria-labelledby={titleId}
        tabIndex={-1}
        className="fixed inset-y-0 right-0 z-40 flex min-h-0 w-full flex-col gap-4 border-l bg-background p-4 text-sm shadow-lg outline-none duration-200 animate-in slide-in-from-right md:w-[var(--pane-width)]"
        style={{ "--pane-width": `${width}px` } as CSSProperties}
        onKeyDown={(event) => {
          trapTab(event)
          onKeyDown?.(event)
        }}
        {...props}
        onFocusCapture={(event) => {
          focused.current = { pane: event.currentTarget, control: event.target }
          onFocusCapture?.(event)
        }}
      >
        <div className="-mb-2 flex shrink-0 justify-end">
          <Button
            data-slot="pane-close"
            variant="ghost"
            size="icon-sm"
            onClick={onClose}
            aria-label="Close"
          >
            <XIcon />
          </Button>
        </div>
        {children}
        <div
          role="separator"
          aria-label="Resize details"
          aria-orientation="vertical"
          aria-valuemin={minimum}
          aria-valuemax={maximum}
          aria-valuenow={width}
          tabIndex={0}
          className="absolute inset-y-0 -left-1 z-10 hidden w-2 cursor-col-resize touch-none hover:bg-primary/20 focus-visible:bg-primary/20 focus-visible:outline-none md:block"
          onPointerDown={(event) => {
            if (event.button !== 0) return
            event.preventDefault()
            event.currentTarget.setPointerCapture?.(event.pointerId)
            drag.current = { pointer: event.pointerId, x: event.clientX, from: width, to: null }
          }}
          onPointerMove={(event) => {
            // One pointer drags; a second finger on the handle moves nothing.
            const current = drag.current
            if (current?.pointer !== event.pointerId) return
            current.to = clamp(current.from + current.x - event.clientX)
            setDragWidth(current.to)
          }}
          onPointerUp={(event) => {
            if (drag.current?.pointer === event.pointerId) endDrag()
          }}
          onPointerCancel={(event) => {
            if (drag.current?.pointer === event.pointerId) endDrag()
          }}
          onDoubleClick={() => setWidth(clamp(36 * rem))}
          onKeyDown={(event) => {
            const next = {
              ArrowLeft: width + 16,
              ArrowRight: width - 16,
              Home: minimum,
              End: maximum,
            }[event.key]
            if (next === undefined) return
            event.preventDefault()
            setWidth(clamp(next))
          }}
        />
      </div>
    </PaneTitleId.Provider>
  )
}

export function PaneHeader({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("flex shrink-0 flex-col gap-1.5", className)} {...props} />
}

export function PaneTitle({ className, ...props }: ComponentProps<"h2">) {
  const id = useContext(PaneTitleId)
  return (
    <h2
      id={id}
      className={cn("font-heading text-base leading-snug font-semibold", className)}
      {...props}
    />
  )
}

/** The child owns its layout; in particular, a session fills this height. */
export function PaneBody({ children }: { children: React.ReactNode }) {
  return (
    <div data-slot="pane-body" className="min-h-0 flex-1 overflow-y-auto">
      <div className="flex h-full flex-col gap-4">{children}</div>
    </div>
  )
}
