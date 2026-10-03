/**
 * The inspector's frame, in the shell's row beside the screen. It takes no
 * focus trap or backdrop: the board keeps working while its details are open.
 * Only a narrow window puts it over the screen instead of beside it.
 */

import { XIcon } from "lucide-react"
import {
  type ComponentProps,
  type CSSProperties,
  createContext,
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

export function DockedPane({
  onClose,
  children,
  hidden,
  ...props
}: ComponentProps<"div"> & { onClose: () => void }) {
  const titleId = useId()
  const savedWidth = useSettingsStore((state) => state.panelWidth)
  const setWidth = useSettingsStore((state) => state.setPanelWidth)
  const [viewport, setViewport] = useState(window.innerWidth)
  const endDrag = useRef<(() => void) | null>(null)
  const rem = Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
  const minimum = 24 * rem
  const maximum = Math.max(minimum, viewport * 0.6)
  const clamp = (value: number) => Math.min(maximum, Math.max(minimum, value))
  const width = clamp(savedWidth)

  useEffect(() => {
    const resize = () => setViewport(window.innerWidth)
    window.addEventListener("resize", resize)
    return () => {
      window.removeEventListener("resize", resize)
      endDrag.current?.()
    }
  }, [])

  return (
    <PaneTitleId.Provider value={titleId}>
      <div
        data-slot="docked-pane"
        role="region"
        aria-labelledby={titleId}
        tabIndex={-1}
        hidden={hidden}
        className={cn(
          "fixed inset-0 z-40 flex min-h-0 w-full shrink-0 flex-col gap-4 border-l bg-background p-4 text-sm outline-none md:relative md:inset-auto md:z-auto md:w-[var(--pane-width)]",
          hidden && "hidden",
        )}
        style={{ "--pane-width": `${width}px` } as CSSProperties}
        {...props}
      >
        <div
          role="separator"
          aria-label="Resize details"
          aria-orientation="vertical"
          aria-valuemin={minimum}
          aria-valuemax={maximum}
          aria-valuenow={width}
          tabIndex={0}
          className="absolute inset-y-0 -left-1 z-10 hidden w-2 cursor-col-resize touch-none hover:bg-primary/20 focus-visible:bg-primary/20 focus-visible:outline-none md:block"
          onMouseDown={(event) => {
            if (event.button !== 0) return
            event.preventDefault()
            endDrag.current?.()
            const start = event.clientX
            const move = (next: MouseEvent) => setWidth(clamp(width + start - next.clientX))
            const end = () => {
              window.removeEventListener("mousemove", move)
              window.removeEventListener("mouseup", end)
              endDrag.current = null
            }
            endDrag.current = end
            window.addEventListener("mousemove", move)
            window.addEventListener("mouseup", end)
          }}
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
        {children}
        <Button
          variant="ghost"
          size="icon-sm"
          className="absolute top-2 right-2"
          onClick={onClose}
          aria-label="Close"
        >
          <XIcon />
        </Button>
      </div>
    </PaneTitleId.Provider>
  )
}

export function PaneHeader({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("flex shrink-0 flex-col gap-1.5 pr-8", className)} {...props} />
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
