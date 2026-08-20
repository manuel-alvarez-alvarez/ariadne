"use client"

import { Dialog as DialogPrimitive } from "@base-ui/react/dialog"
import { XIcon } from "lucide-react"
import type * as React from "react"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * The darkening behind a dialog or a sheet, the one way for both.
 *
 * The blur carries most of it where it is supported, but it cannot be the
 * whole of it: `backdrop-filter` is the first thing a browser drops, and a
 * tenth of black over an already-dark page is a scrim nobody can see. So dark
 * mode takes a real one — the page has to visibly recede, or a modal reads as
 * part of the screen it is covering.
 */
export const SCRIM = "bg-black/10 supports-backdrop-filter:backdrop-blur-xs dark:bg-black/40"

/**
 * A dialog dismisses freely: an outside press and Escape both close it, which
 * is right for everything that only shows something. A dialog holding a form
 * uses `FormDialog` instead, which asks before throwing away what was typed.
 */
function Dialog({ ...props }: DialogPrimitive.Root.Props) {
  return <DialogPrimitive.Root data-slot="dialog" {...props} />
}

function DialogTrigger({ ...props }: DialogPrimitive.Trigger.Props) {
  return <DialogPrimitive.Trigger data-slot="dialog-trigger" {...props} />
}

function DialogPortal({ ...props }: DialogPrimitive.Portal.Props) {
  return <DialogPrimitive.Portal data-slot="dialog-portal" {...props} />
}

function DialogClose({ ...props }: DialogPrimitive.Close.Props) {
  return <DialogPrimitive.Close data-slot="dialog-close" {...props} />
}

function DialogOverlay({ className, ...props }: DialogPrimitive.Backdrop.Props) {
  return (
    <DialogPrimitive.Backdrop
      data-slot="dialog-overlay"
      className={cn(
        "fixed inset-0 isolate z-50 duration-100 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0",
        SCRIM,
        className,
      )}
      {...props}
    />
  )
}

/**
 * The popup centers itself with `inset-0 m-auto h-fit`, not with the usual
 * `top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2`. Half of an odd number
 * is a half pixel, so that translate hands the browser a surface sitting
 * between two device pixels, and a translated surface is not snapped the way a
 * laid-out one is: the ring and the hairlines get drawn across two rows at
 * half strength, for as long as the dialog is open. Auto margins center the
 * same box in layout instead, so the settled dialog carries no transform at
 * all. The enter and exit animations still scale it, which is fine — that is
 * over before the dialog is anything to read.
 *
 * The popup is a flex column rather than a grid for a second reason of the
 * same kind: WebKit sizes an out-of-flow `height: fit-content` grid container
 * — which every dialog here is — to the whole space it could have taken, so
 * the box ends up viewport-tall and its rows stretch, leaving craters between
 * the header, the fields and the footer. Chromium hugs either way. A flex
 * column hugs its content in both engines, and caps and scrolls in both when a
 * caller adds a `max-h-*`.
 */
function DialogContent({
  className,
  children,
  showCloseButton = true,
  ...props
}: DialogPrimitive.Popup.Props & {
  showCloseButton?: boolean
}) {
  return (
    <DialogPortal>
      <DialogOverlay />
      <DialogPrimitive.Popup
        data-slot="dialog-content"
        className={cn(
          "fixed inset-0 z-50 m-auto flex h-fit w-full max-w-[calc(100%-2rem)] flex-col gap-3 rounded-xl bg-popover p-4 text-sm text-popover-foreground ring-1 ring-foreground/10 duration-100 outline-none sm:max-w-sm data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
          className,
        )}
        {...props}
      >
        {children}
        {showCloseButton && (
          <DialogPrimitive.Close
            data-slot="dialog-close"
            render={<Button variant="ghost" className="absolute top-2 right-2" size="icon-sm" />}
          >
            <XIcon />
            <span className="sr-only">Close</span>
          </DialogPrimitive.Close>
        )}
      </DialogPrimitive.Popup>
    </DialogPortal>
  )
}

function DialogHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div data-slot="dialog-header" className={cn("flex flex-col gap-1.5", className)} {...props} />
  )
}

function DialogFooter({
  className,
  showCloseButton = false,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  showCloseButton?: boolean
}) {
  return (
    <div
      data-slot="dialog-footer"
      className={cn(
        "-mx-4 -mb-4 flex flex-col-reverse gap-2 rounded-b-xl border-t bg-muted/50 px-4 py-3 sm:flex-row sm:justify-end",
        className,
      )}
      {...props}
    >
      {children}
      {showCloseButton && (
        <DialogPrimitive.Close render={<Button variant="outline" />}>Close</DialogPrimitive.Close>
      )}
    </div>
  )
}

function DialogTitle({ className, ...props }: DialogPrimitive.Title.Props) {
  return (
    <DialogPrimitive.Title
      data-slot="dialog-title"
      className={cn("font-heading text-base leading-none font-medium", className)}
      {...props}
    />
  )
}

function DialogDescription({ className, ...props }: DialogPrimitive.Description.Props) {
  return (
    <DialogPrimitive.Description
      data-slot="dialog-description"
      className={cn(
        "text-sm text-muted-foreground *:[a]:underline *:[a]:underline-offset-3 *:[a]:hover:text-foreground",
        className,
      )}
      {...props}
    />
  )
}

export {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogOverlay,
  DialogPortal,
  DialogTitle,
  DialogTrigger,
}
