/**
 * The colours a host can have: the eight of `HostDto.color`, their labels in a
 * fixed order for a colour picker, and the classes that draw each one.
 */

import type { HostDto } from "@/api/types"

export type HostColor = NonNullable<HostDto["color"]>

/** What a host is drawn in: one of the eight colours, or the neutral grey. */
export type HostHue = HostColor | "grey"

/** The eight colours in their fixed order, with the labels a colour picker shows. */
export const HOST_COLORS: readonly { value: HostColor; label: string }[] = [
  { value: "red", label: "Red" },
  { value: "orange", label: "Orange" },
  { value: "amber", label: "Amber" },
  { value: "green", label: "Green" },
  { value: "teal", label: "Teal" },
  { value: "blue", label: "Blue" },
  { value: "violet", label: "Violet" },
  { value: "pink", label: "Pink" },
]

/** Tailwind classes per hue, spelled out so the compiler sees each one. */
export const HOST_HUE_CLASSES: Record<HostHue, { dot: string; pill: string }> = {
  red: { dot: "bg-host-red", pill: "bg-host-red-soft text-host-red-fg" },
  orange: { dot: "bg-host-orange", pill: "bg-host-orange-soft text-host-orange-fg" },
  amber: { dot: "bg-host-amber", pill: "bg-host-amber-soft text-host-amber-fg" },
  green: { dot: "bg-host-green", pill: "bg-host-green-soft text-host-green-fg" },
  teal: { dot: "bg-host-teal", pill: "bg-host-teal-soft text-host-teal-fg" },
  blue: { dot: "bg-host-blue", pill: "bg-host-blue-soft text-host-blue-fg" },
  violet: { dot: "bg-host-violet", pill: "bg-host-violet-soft text-host-violet-fg" },
  pink: { dot: "bg-host-pink", pill: "bg-host-pink-soft text-host-pink-fg" },
  grey: { dot: "bg-host-grey", pill: "bg-host-grey-soft text-host-grey-fg" },
}
