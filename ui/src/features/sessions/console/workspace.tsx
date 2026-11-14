import { XIcon } from "lucide-react"
import { type ReactNode, type RefObject, useLayoutEffect, useRef, useState } from "react"
import { Button } from "@/components/ui/button"
import { Sheet, SheetContent, SheetTitle } from "@/components/ui/sheet"
import { BlockView } from "./blocks"
import type { Block } from "./transcript"

const keepDetailsOpen = () => {}

/** Both console sizes use the available panel width, not the browser width. */
export function ConsoleWorkspace({
  children,
  tool,
  onClose,
  trigger,
  pendingControl,
  returnTarget,
}: {
  children: ReactNode
  tool: Extract<Block, { type: "tool" }> | undefined
  onClose: () => void
  trigger: RefObject<HTMLElement | null>
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
  const id = tool?.id
  useLayoutEffect(() => {
    if (id && wide) {
      close.current?.focus()
      if (detailsScroll.current) detailsScroll.current.scrollTop = 0
    }
  }, [id, wide])
  const restoreFocus = () =>
    (returnTarget() ?? (trigger.current?.isConnected ? trigger.current : root.current))?.focus()
  function dismiss() {
    onClose()
    restoreFocus()
  }
  const content = tool ? (
    <BlockView block={tool} picker={null} folded={false} onToggle={keepDetailsOpen} />
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
            aria-label="Tool details"
            className="flex min-h-0 w-[45%] min-w-0 flex-col border-l bg-muted/10"
            onKeyDownCapture={(event) => {
              if (event.key === "Escape" && !event.metaKey) {
                event.preventDefault()
                event.stopPropagation()
                dismiss()
              }
            }}
          >
            <div className="flex items-center justify-between border-b px-4 py-2">
              <h3 className="font-medium">Tool details</h3>
              <Button
                ref={close}
                size="icon-xs"
                variant="ghost"
                onClick={dismiss}
                aria-label="Close tool details"
              >
                <XIcon />
              </Button>
            </div>
            <div ref={detailsScroll} className="min-h-0 flex-1 overflow-y-auto p-4">
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
              <SheetTitle>Tool details</SheetTitle>
              <Button
                size="icon-xs"
                variant="ghost"
                onClick={onClose}
                aria-label="Close tool details"
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
