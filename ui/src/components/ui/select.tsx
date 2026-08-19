import { Select as SelectPrimitive } from "@base-ui/react/select"
import { CheckIcon, ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import type * as React from "react"
import { useEffect, useState } from "react"
import { cn } from "@/lib/utils"

const Select = SelectPrimitive.Root

/**
 * Keeps the open popup on the device pixel grid, so its text is not resampled.
 *
 * Every other anchored surface in the app comes out of Floating UI, which
 * rounds the coordinates it hands the popup to whole device pixels before
 * writing them (`roundByDPR`) — dropdown menus, submenus and tooltips all
 * settle on whole pixels for that reason. The select is the exception:
 * `alignItemWithTrigger` (on by default, and what makes the selected item open
 * *over* the trigger's own text) replaces those coordinates with its own, taken
 * from the distance between the trigger's value text and the item's text, and
 * writes them onto the positioner unrounded. A trigger sitting on a subpixel
 * boundary — which any flex row with proportional text puts it on — therefore
 * parks the whole popup half a pixel off, where its hairline ring and its
 * shadow are split across two rows of pixels at half strength each and its
 * text is resampled rather than drawn.
 *
 * So the popup is measured after Base UI has positioned it and nudged back onto
 * the grid, by editing the same inline `left`/`top`/`bottom` it wrote. The
 * correction is under one device pixel, which is far below what the alignment
 * it undoes was worth. A `MutationObserver` rather than a one-shot pass because
 * Base UI repositions again while the popup is open — when the list mounts, for
 * one — and its write has to be the one that gets corrected, not the first one;
 * the callback is a microtask, so the fix still lands before the frame paints.
 * Correcting is idempotent: a popup already on the grid is measured, found
 * whole, and left alone, which is also what makes the observer terminate.
 */
function usePixelSnappedPositioner() {
  // A callback ref rather than `useRef`: the positioner is portalled in when the
  // select opens, and a ref object does not re-render to say it arrived.
  const [positioner, setPositioner] = useState<HTMLDivElement | null>(null)

  useEffect(() => {
    if (!positioner) return

    function snap() {
      if (!positioner) return
      const dpr = window.devicePixelRatio || 1
      const grid = (value: number) => Math.round(value * dpr) / dpr
      const rect = positioner.getBoundingClientRect()
      const dx = grid(rect.x) - rect.x
      const dy = grid(rect.y) - rect.y
      if (dx === 0 && dy === 0) return

      // Whichever edge Base UI anchored the popup by: `top` when it hangs down
      // from one, `bottom` when it is pinned to the viewport's floor and grows
      // upwards. Moving the box down means giving `bottom` that much less.
      const { style } = positioner
      if (dx !== 0 && style.left) style.left = `${Number.parseFloat(style.left) + dx}px`
      if (dy !== 0 && style.top) style.top = `${Number.parseFloat(style.top) + dy}px`
      else if (dy !== 0 && style.bottom) style.bottom = `${Number.parseFloat(style.bottom) - dy}px`
    }

    snap()
    const observer = new MutationObserver(snap)
    observer.observe(positioner, { attributeFilter: ["style"] })
    return () => observer.disconnect()
  }, [positioner])

  return setPositioner
}

function SelectGroup({ className, ...props }: SelectPrimitive.Group.Props) {
  return (
    <SelectPrimitive.Group
      data-slot="select-group"
      className={cn("scroll-my-1 p-1", className)}
      {...props}
    />
  )
}

function SelectValue({ className, ...props }: SelectPrimitive.Value.Props) {
  return (
    <SelectPrimitive.Value
      data-slot="select-value"
      className={cn("flex flex-1 text-left", className)}
      {...props}
    />
  )
}

function SelectTrigger({
  className,
  size = "default",
  children,
  ...props
}: SelectPrimitive.Trigger.Props & {
  size?: "sm" | "default"
}) {
  return (
    <SelectPrimitive.Trigger
      data-slot="select-trigger"
      data-size={size}
      className={cn(
        "flex w-fit items-center justify-between gap-1.5 rounded-lg border border-input bg-transparent py-2 pr-2 pl-2.5 text-sm whitespace-nowrap transition-colors outline-none select-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20 data-placeholder:text-muted-foreground data-[size=default]:h-8 data-[size=sm]:h-7 data-[size=sm]:rounded-[min(var(--radius-md),10px)] *:data-[slot=select-value]:line-clamp-1 *:data-[slot=select-value]:flex *:data-[slot=select-value]:items-center *:data-[slot=select-value]:gap-1.5 dark:bg-input/30 dark:hover:bg-input/50 dark:aria-invalid:border-destructive/50 dark:aria-invalid:ring-destructive/40 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
        className,
      )}
      {...props}
    >
      {children}
      <SelectPrimitive.Icon
        render={<ChevronDownIcon className="pointer-events-none size-4 text-muted-foreground" />}
      />
    </SelectPrimitive.Trigger>
  )
}

function SelectContent({
  className,
  children,
  side = "bottom",
  sideOffset = 4,
  align = "center",
  alignOffset = 0,
  alignItemWithTrigger = true,
  ...props
}: SelectPrimitive.Popup.Props &
  Pick<
    SelectPrimitive.Positioner.Props,
    "align" | "alignOffset" | "side" | "sideOffset" | "alignItemWithTrigger"
  >) {
  const positionerRef = usePixelSnappedPositioner()

  return (
    <SelectPrimitive.Portal>
      <SelectPrimitive.Positioner
        ref={positionerRef}
        side={side}
        sideOffset={sideOffset}
        align={align}
        alignOffset={alignOffset}
        alignItemWithTrigger={alignItemWithTrigger}
        className="isolate z-50"
      >
        <SelectPrimitive.Popup
          data-slot="select-content"
          data-align-trigger={alignItemWithTrigger}
          className={cn(
            "relative isolate z-50 max-h-(--available-height) w-(--anchor-width) min-w-36 origin-(--transform-origin) overflow-x-hidden overflow-y-auto rounded-lg bg-popover text-popover-foreground shadow-md ring-1 ring-foreground/10 duration-100 data-[align-trigger=true]:animate-none data-[side=bottom]:slide-in-from-top-2 data-[side=inline-end]:slide-in-from-left-2 data-[side=inline-start]:slide-in-from-right-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95",
            className,
          )}
          {...props}
        >
          <SelectScrollUpButton />
          <SelectPrimitive.List>{children}</SelectPrimitive.List>
          <SelectScrollDownButton />
        </SelectPrimitive.Popup>
      </SelectPrimitive.Positioner>
    </SelectPrimitive.Portal>
  )
}

function SelectLabel({ className, ...props }: SelectPrimitive.GroupLabel.Props) {
  return (
    <SelectPrimitive.GroupLabel
      data-slot="select-label"
      className={cn("px-1.5 py-1 text-xs text-muted-foreground", className)}
      {...props}
    />
  )
}

function SelectItem({ className, children, ...props }: SelectPrimitive.Item.Props) {
  return (
    <SelectPrimitive.Item
      data-slot="select-item"
      className={cn(
        "relative flex w-full cursor-default items-center gap-1.5 rounded-md py-1 pr-8 pl-1.5 text-sm outline-hidden select-none focus:bg-accent focus:text-accent-foreground not-data-[variant=destructive]:focus:**:text-accent-foreground data-disabled:pointer-events-none data-disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4 *:[span]:last:flex *:[span]:last:items-center *:[span]:last:gap-2",
        className,
      )}
      {...props}
    >
      <SelectPrimitive.ItemText className="flex flex-1 shrink-0 gap-2 whitespace-nowrap">
        {children}
      </SelectPrimitive.ItemText>
      <SelectPrimitive.ItemIndicator
        render={
          <span className="pointer-events-none absolute right-2 flex size-4 items-center justify-center" />
        }
      >
        <CheckIcon className="pointer-events-none" />
      </SelectPrimitive.ItemIndicator>
    </SelectPrimitive.Item>
  )
}

function SelectSeparator({ className, ...props }: SelectPrimitive.Separator.Props) {
  return (
    <SelectPrimitive.Separator
      data-slot="select-separator"
      className={cn("pointer-events-none -mx-1 my-1 h-px bg-border", className)}
      {...props}
    />
  )
}

function SelectScrollUpButton({
  className,
  ...props
}: React.ComponentProps<typeof SelectPrimitive.ScrollUpArrow>) {
  return (
    <SelectPrimitive.ScrollUpArrow
      data-slot="select-scroll-up-button"
      className={cn(
        "top-0 z-10 flex w-full cursor-default items-center justify-center bg-popover py-1 [&_svg:not([class*='size-'])]:size-4",
        className,
      )}
      {...props}
    >
      <ChevronUpIcon />
    </SelectPrimitive.ScrollUpArrow>
  )
}

function SelectScrollDownButton({
  className,
  ...props
}: React.ComponentProps<typeof SelectPrimitive.ScrollDownArrow>) {
  return (
    <SelectPrimitive.ScrollDownArrow
      data-slot="select-scroll-down-button"
      className={cn(
        "bottom-0 z-10 flex w-full cursor-default items-center justify-center bg-popover py-1 [&_svg:not([class*='size-'])]:size-4",
        className,
      )}
      {...props}
    >
      <ChevronDownIcon />
    </SelectPrimitive.ScrollDownArrow>
  )
}

export {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectScrollDownButton,
  SelectScrollUpButton,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
}
