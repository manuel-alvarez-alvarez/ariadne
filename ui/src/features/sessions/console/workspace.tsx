import { XIcon } from "lucide-react"
import {
  type ReactNode,
  type RefObject,
  useCallback,
  useLayoutEffect,
  useRef,
  useState,
} from "react"
import { Button } from "@/components/ui/button"
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet"
import { BlockView } from "./blocks"
import type { Block } from "./transcript"

/** Both console sizes use the available panel width, not the browser width. */
export function ConsoleWorkspace({
  children,
  tool,
  onClose,
  trigger,
  pendingControl,
  returnTarget,
  picker,
  requestReveal,
}: {
  children: ReactNode
  tool: Extract<Block, { type: "tool" | "permission" }> | undefined
  onClose: () => void
  trigger: RefObject<HTMLElement | null>
  requestReveal: number
  picker?: ReactNode
  pendingControl: ReactNode
  returnTarget: () => HTMLElement | null
}) {
  const root = useRef<HTMLDivElement>(null)
  const close = useRef<HTMLButtonElement>(null)
  const detailsScroll = useRef<HTMLDivElement>(null)
  const [wide, setWide] = useState(false)
  useLayoutEffect(() => {
    const element = root.current
    if (!element) return
    setWide(element.getBoundingClientRect().width >= 880)
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWide(entry.contentRect.width >= 880)
    })
    observer.observe(element)
    return () => observer.disconnect()
  }, [])
  const [detailsFolded, setDetailsFolded] = useState(false)
  const toggleDetails = useCallback(() => setDetailsFolded((value) => !value), [])
  const title = tool?.type === "permission" ? "Permission details" : "Tool details"
  const closeLabel = `Close ${title.toLowerCase()}`
  const focusDetails = () =>
    (
      detailsScroll.current?.querySelector<HTMLElement>('[role="listbox"]') ?? close.current
    )?.focus()
  const lastSelected = useRef<string | undefined>(undefined)
  if (tool) lastSelected.current = tool.id
  const id = tool?.id
  // biome-ignore lint/correctness/useExhaustiveDependencies: focus follows a deliberate selection or layout change
  useLayoutEffect(() => {
    setDetailsFolded(false)
    if (id) {
      focusDetails()
      if (detailsScroll.current) detailsScroll.current.scrollTop = 0
    }
  }, [id, wide, requestReveal])
  const restoreFocus = () => {
    const row = Array.from(root.current?.querySelectorAll<HTMLElement>("[data-block-id]") ?? [])
      .find((element) => element.dataset.blockId === lastSelected.current)
      ?.querySelector<HTMLButtonElement>("button")
    const target =
      returnTarget() ?? (trigger.current?.isConnected ? trigger.current : (row ?? root.current))
    target?.focus()
  }
  function dismiss() {
    onClose()
    restoreFocus()
  }
  const content = tool ? (
    <BlockView
      block={tool}
      picker={picker ?? null}
      folded={detailsFolded}
      onToggle={toggleDetails}
    />
  ) : null
  return (
    <div
      ref={root}
      data-console-workspace
      tabIndex={-1}
      className="flex min-h-0 flex-1 outline-none"
    >
      <div className="flex min-h-0 min-w-0 flex-1 flex-col">{children}</div>
      {wide ? (
        tool ? (
          <aside
            aria-label={title}
            className="flex min-h-0 w-[40%] min-w-0 flex-col border-l bg-muted/10"
            onKeyDownCapture={(event) => {
              if (event.key === "Escape" && !event.metaKey) {
                event.preventDefault()
                event.stopPropagation()
                dismiss()
              }
            }}
          >
            <div className="flex items-center justify-between border-b px-3 py-1.5">
              <h3 className="font-medium">{title}</h3>
              <Button
                ref={close}
                size="icon-xs"
                variant="ghost"
                onClick={dismiss}
                aria-label={closeLabel}
              >
                <XIcon />
              </Button>
            </div>
            <div ref={detailsScroll} className="min-h-0 flex-1 overflow-y-auto p-3">
              {content}
            </div>
          </aside>
        ) : null
      ) : (
        <Sheet
          open={tool !== undefined}
          onOpenChange={(open) => {
            if (!open) onClose()
          }}
        >
          <SheetContent
            ref={detailsScroll}
            initialFocus={() =>
              detailsScroll.current?.querySelector<HTMLElement>('[role="listbox"]') ?? close.current
            }
            showCloseButton={false}
            finalFocus={() => {
              restoreFocus()
              return false
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") event.stopPropagation()
            }}
          >
            <div className="flex items-center justify-between">
              <SheetTitle>{title}</SheetTitle>
              <Button
                ref={close}
                size="icon-xs"
                variant="ghost"
                onClick={onClose}
                aria-label={closeLabel}
              >
                <XIcon />
              </Button>
            </div>
            {pendingControl}
            {content}
          </SheetContent>
        </Sheet>
      )}
    </div>
  )
}
