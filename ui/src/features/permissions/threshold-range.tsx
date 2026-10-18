/**
 * The allow/deny threshold pair as one range control: a two-handle slider
 * over three coloured zones, the zone labels in a row above the track, and
 * the two editable numbers in a row below it — held to the card's edges
 * rather than under the handles, so neither is cut off at 0 or 1 nor laid
 * over the other when the thresholds are close. Both surfaces read and write the same
 * pair, so a drag and a keystroke move each other in step, and neither
 * handle can pass the other.
 *
 * A commit — a handle released, or a number field left or Entered — sends
 * only the one field that changed, clamped to 0–1. A refusal is toasted with
 * the daemon's own message, and the row snaps back to the value it still
 * holds: the buffer here is never the truth, `allowThreshold`/`denyThreshold`
 * are.
 */

import { Slider as SliderPrimitive } from "@base-ui/react/slider"
import { useEffect, useState } from "react"
import { toast } from "sonner"

import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { describeError } from "@/lib/format"

import type { useUpdateAiPermissions } from "./queries"

const STEP = 0.01
const INPUT_STEP = 0.0001
const INPUT_DECIMALS = 4

const thumbClassName =
  "relative block size-3.5 shrink-0 rounded-full border border-ring bg-white ring-ring/50 transition-[color,box-shadow] select-none after:absolute after:-inset-2 hover:ring-3 focus-visible:ring-3 focus-visible:outline-hidden active:ring-3"

const numberInputClassName =
  "h-7 w-24 px-2 text-right text-xs tabular-nums [appearance:textfield] [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none"

/** A label centred on `percent` of the track, kept `inset` from either end so
 * its text never runs past the card. */
function insetLeft(percent: number, inset = "1.75rem"): string {
  return `clamp(${inset}, ${percent}%, calc(100% - ${inset}))`
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value))
}

function formatDecimals(value: number): string {
  return value.toFixed(INPUT_DECIMALS)
}

/** A typed field's raw text, read as a number the way the one below reads it. */
function readTyped(text: string): number | null {
  if (text.trim() === "") {
    return null
  }
  const value = Number(text)
  return Number.isNaN(value) ? null : value
}

/** What a typed field commits: the step attribute alone does not stop a
 * pasted or spun-past value from carrying more, so round it here instead. */
function roundToInputDecimals(value: number): number {
  const factor = 10 ** INPUT_DECIMALS
  return Math.round(value * factor) / factor
}

export function ThresholdRange({
  allowThreshold,
  denyThreshold,
  danger,
  mutate,
}: {
  allowThreshold: number
  denyThreshold: number
  danger?: number
  mutate: ReturnType<typeof useUpdateAiPermissions>["mutate"]
}) {
  const [allowValue, setAllowValue] = useState(allowThreshold)
  useEffect(() => setAllowValue(allowThreshold), [allowThreshold])
  const [denyValue, setDenyValue] = useState(denyThreshold)
  useEffect(() => setDenyValue(denyThreshold), [denyThreshold])

  const [allowText, setAllowText] = useState(formatDecimals(allowThreshold))
  useEffect(() => setAllowText(formatDecimals(allowThreshold)), [allowThreshold])
  const [denyText, setDenyText] = useState(formatDecimals(denyThreshold))
  useEffect(() => setDenyText(formatDecimals(denyThreshold)), [denyThreshold])

  function commitAllow(value: number) {
    mutate(
      { allow_threshold: value },
      {
        onError: (error) => {
          toast.error("Could not change the allow threshold", { description: describeError(error) })
          setAllowValue(allowThreshold)
          setAllowText(formatDecimals(allowThreshold))
        },
      },
    )
  }

  function commitDeny(value: number) {
    mutate(
      { deny_threshold: value },
      {
        onError: (error) => {
          toast.error("Could not change the deny threshold", { description: describeError(error) })
          setDenyValue(denyThreshold)
          setDenyText(formatDecimals(denyThreshold))
        },
      },
    )
  }

  function handleSliderChange(value: readonly [number, number]) {
    const [allow, deny] = value
    setAllowValue(allow)
    setAllowText(formatDecimals(allow))
    setDenyValue(deny)
    setDenyText(formatDecimals(deny))
  }

  function handleSliderCommit(value: readonly [number, number]) {
    const [allow, deny] = value
    setAllowText(formatDecimals(allow))
    setDenyText(formatDecimals(deny))
    if (allow !== allowThreshold) {
      commitAllow(allow)
    }
    if (deny !== denyThreshold) {
      commitDeny(deny)
    }
  }

  function handleAllowTextChange(text: string) {
    setAllowText(text)
    const parsed = readTyped(text)
    if (parsed !== null) {
      setAllowValue(clamp01(parsed))
    }
  }

  function handleDenyTextChange(text: string) {
    setDenyText(text)
    const parsed = readTyped(text)
    if (parsed !== null) {
      setDenyValue(clamp01(parsed))
    }
  }

  function commitAllowText() {
    const parsed = readTyped(allowText)
    if (parsed !== null) {
      const rounded = roundToInputDecimals(clamp01(parsed))
      setAllowText(formatDecimals(rounded))
      commitAllow(rounded)
    }
  }

  function commitDenyText() {
    const parsed = readTyped(denyText)
    if (parsed !== null) {
      const rounded = roundToInputDecimals(clamp01(parsed))
      setDenyText(formatDecimals(rounded))
      commitDeny(rounded)
    }
  }

  function blurOnEnter(event: React.KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.currentTarget.blur()
    }
  }

  const allowPercent = clamp01(allowValue) * 100
  const denyPercent = clamp01(denyValue) * 100
  const zoneStart = Math.min(allowPercent, denyPercent)
  const zoneEnd = Math.max(allowPercent, denyPercent)

  const dangerPercent = danger !== undefined ? clamp01(danger) * 100 : undefined
  const minZoneWidthForLabel = 12
  // A zone label gives way to the danger readout that shares its row.
  const zones = [
    { label: "Allow", center: zoneStart / 2, width: zoneStart },
    { label: "Ask", center: (zoneStart + zoneEnd) / 2, width: zoneEnd - zoneStart },
    { label: "Deny", center: (zoneEnd + 100) / 2, width: 100 - zoneEnd },
  ].map((zone) => ({
    ...zone,
    shown:
      zone.width >= minZoneWidthForLabel &&
      (dangerPercent === undefined || Math.abs(zone.center - dangerPercent) > 10),
  }))

  return (
    <div className="flex flex-col gap-1.5">
      <div className="relative h-4 text-xs text-muted-foreground">
        {zones.map((zone) => (
          <span
            key={zone.label}
            data-slot="threshold-zone-label"
            aria-hidden={!zone.shown}
            className={`pointer-events-none absolute top-0 -translate-x-1/2 transition-opacity ${zone.shown ? "opacity-100" : "opacity-0"}`}
            style={{ left: `${zone.center}%` }}
          >
            {zone.label}
          </span>
        ))}
        {dangerPercent !== undefined ? (
          <span
            aria-hidden="true"
            className="pointer-events-none absolute top-0 -translate-x-1/2 font-medium text-foreground tabular-nums"
            style={{ left: insetLeft(dangerPercent) }}
          >
            {formatDecimals(dangerPercent / 100)}
          </span>
        ) : null}
      </div>
      <SliderPrimitive.Root
        value={[allowValue, denyValue]}
        min={0}
        max={1}
        step={STEP}
        minStepsBetweenValues={1}
        thumbCollisionBehavior="none"
        onValueChange={(value) => handleSliderChange(value as [number, number])}
        onValueCommitted={(value) => handleSliderCommit(value as [number, number])}
      >
        <SliderPrimitive.Control className="relative flex w-full touch-none items-center py-1.5 select-none">
          <SliderPrimitive.Track
            data-slot="slider-track"
            className="relative h-1.5 w-full grow rounded-full select-none"
          >
            <div className="pointer-events-none absolute inset-0 overflow-hidden rounded-full">
              <div
                className="absolute inset-y-0 left-0 bg-status-done"
                style={{ width: `${zoneStart}%` }}
              />
              <div
                className="absolute inset-y-0 bg-status-warn"
                style={{ left: `${zoneStart}%`, width: `${zoneEnd - zoneStart}%` }}
              />
              <div
                className="absolute inset-y-0 right-0 bg-status-danger"
                style={{ left: `${zoneEnd}%` }}
              />
            </div>
            {dangerPercent !== undefined ? (
              <div
                role="img"
                aria-label={`Danger ${formatDecimals(dangerPercent / 100)}`}
                className="pointer-events-none absolute top-1/2 h-3 w-0.5 -translate-x-1/2 -translate-y-1/2 bg-foreground"
                style={{ left: `${dangerPercent}%` }}
              />
            ) : null}
            <SliderPrimitive.Thumb
              index={0}
              aria-label="Allow threshold"
              aria-valuetext={`Allow threshold ${formatDecimals(allowValue)}`}
              className={thumbClassName}
            />
            <SliderPrimitive.Thumb
              index={1}
              aria-label="Deny threshold"
              aria-valuetext={`Deny threshold ${formatDecimals(denyValue)}`}
              className={thumbClassName}
            />
          </SliderPrimitive.Track>
        </SliderPrimitive.Control>
      </SliderPrimitive.Root>

      <div
        data-slot="threshold-inputs"
        className="flex flex-wrap items-center justify-between gap-3"
      >
        <div className="flex items-center gap-2">
          <Label htmlFor="ai-allow-threshold" className="gap-1.5 text-xs font-normal">
            <span aria-hidden="true" className="size-2 shrink-0 rounded-full bg-status-done" />
            <span aria-hidden="true">Allow up to</span>
            <span className="sr-only">Allow threshold</span>
          </Label>
          <Input
            id="ai-allow-threshold"
            type="number"
            min={0}
            max={1}
            step={INPUT_STEP}
            value={allowText}
            className={numberInputClassName}
            onChange={(event) => handleAllowTextChange(event.target.value)}
            onBlur={commitAllowText}
            onKeyDown={blurOnEnter}
          />
        </div>
        <div className="flex items-center gap-2">
          <Label htmlFor="ai-deny-threshold" className="gap-1.5 text-xs font-normal">
            <span aria-hidden="true" className="size-2 shrink-0 rounded-full bg-status-danger" />
            <span aria-hidden="true">Deny from</span>
            <span className="sr-only">Deny threshold</span>
          </Label>
          <Input
            id="ai-deny-threshold"
            type="number"
            min={0}
            max={1}
            step={INPUT_STEP}
            value={denyText}
            className={numberInputClassName}
            onChange={(event) => handleDenyTextChange(event.target.value)}
            onBlur={commitDenyText}
            onKeyDown={blurOnEnter}
          />
        </div>
      </div>
    </div>
  )
}
