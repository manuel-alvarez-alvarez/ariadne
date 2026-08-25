"use client"

import { Dialog as SheetPrimitive } from "@base-ui/react/dialog"
import { XIcon } from "lucide-react"
import type * as React from "react"
import { Button } from "@/components/ui/button"
import { SCRIM } from "@/components/ui/dialog"
import { cn } from "@/lib/format"

/**
 * A side panel: the dialog primitive, but pinned to one edge of the viewport —
 * the right edge and full-height for detail views that open over the screen
 * they came from, or the bottom edge and full-width for a drawer.
 */

function Sheet({ ...props }: SheetPrimitive.Root.Props) {
  return <SheetPrimitive.Root data-slot="sheet" {...props} />
}

function SheetPortal({ ...props }: SheetPrimitive.Portal.Props) {
  return <SheetPrimitive.Portal data-slot="sheet-portal" {...props} />
}

/**
 * `dim` is what makes a stack of sheets read as one: the darkening belongs to
 * the topmost sheet, and the ones under it go clear so the page is never
 * darkened twice.
 */
function SheetOverlay({
  className,
  dim = true,
  ...props
}: SheetPrimitive.Backdrop.Props & { dim?: boolean }) {
  return (
    <SheetPrimitive.Backdrop
      data-slot="sheet-overlay"
      className={cn(
        "fixed inset-0 isolate z-50 duration-100 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0",
        dim && SCRIM,
        className,
      )}
      {...props}
    />
  )
}

const SHEET_SIDES = {
  right:
    "inset-y-0 right-0 border-l sm:max-w-xl data-open:slide-in-from-right data-closed:slide-out-to-right",
  bottom:
    "inset-x-0 bottom-0 max-h-[80svh] border-t data-open:slide-in-from-bottom data-closed:slide-out-to-bottom",
}

function SheetContent({
  className,
  children,
  showCloseButton = true,
  side = "right",
  overlay,
  ...props
}: SheetPrimitive.Popup.Props & {
  showCloseButton?: boolean
  /** Which edge the sheet is pinned to. */
  side?: keyof typeof SHEET_SIDES
  /**
   * The backdrop, for a sheet that is part of a stack: a nested sheet has none
   * of its own unless it asks (`forceRender`), and the one it opened over
   * gives up its darkening (`dim`) so there is only ever one.
   */
  overlay?: React.ComponentProps<typeof SheetOverlay>
}) {
  return (
    <SheetPortal>
      <SheetOverlay {...overlay} />
      <SheetPrimitive.Popup
        data-slot="sheet-content"
        className={cn(
          // `*:shrink-0`: the popup is a fixed-height flex column, and without
          // it a child carrying `min-h-0` gets squashed to fit the viewport
          // instead of overflowing into the panel's scroll.
          // The trailing `after` spacer is the bottom padding: WebKit does not
          // render a scroll container's own padding-bottom past overflowing
          // content, but a flex item at the end is honoured everywhere.
          "fixed z-50 flex w-full flex-col gap-4 overflow-y-auto bg-background p-4 text-sm text-foreground shadow-lg duration-150 outline-none *:shrink-0 after:block after:h-2 after:shrink-0 data-open:animate-in data-closed:animate-out",
          SHEET_SIDES[side],
          className,
        )}
        {...props}
      >
        {children}
        {showCloseButton && (
          <SheetPrimitive.Close
            data-slot="sheet-close"
            render={<Button variant="ghost" className="absolute top-2 right-2" size="icon-sm" />}
          >
            <XIcon />
            <span className="sr-only">Close</span>
          </SheetPrimitive.Close>
        )}
      </SheetPrimitive.Popup>
    </SheetPortal>
  )
}

function SheetHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="sheet-header"
      className={cn("flex flex-col gap-1.5 pr-8", className)}
      {...props}
    />
  )
}

function SheetTitle({ className, ...props }: SheetPrimitive.Title.Props) {
  return (
    <SheetPrimitive.Title
      data-slot="sheet-title"
      className={cn("font-heading text-base leading-snug font-semibold", className)}
      {...props}
    />
  )
}

function SheetDescription({ className, ...props }: SheetPrimitive.Description.Props) {
  return (
    <SheetPrimitive.Description
      data-slot="sheet-description"
      className={cn("text-sm text-muted-foreground", className)}
      {...props}
    />
  )
}

export { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle }
