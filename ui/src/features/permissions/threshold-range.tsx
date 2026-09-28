/**
 * The allow/deny threshold pair as one range control: a two-handle slider
 * over three coloured zones, and the two number inputs the handles' values
 * are typed into. Both surfaces read and write the same pair, so a drag and
 * a keystroke move each other in step, and neither handle can pass the
 * other.
 *
 * A commit — a handle released, or a number field left or Entered — sends
 * only the one field that changed. A refusal is toasted with the daemon's
 * own message, and the row snaps back to the value it still holds: the
 * buffer here is never the truth, `allowThreshold`/`denyThreshold` are.
 */

import { Slider as SliderPrimitive } from "@base-ui/react/slider"
import { useEffect, useState } from "react"
import { toast } from "sonner"

import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { describeError } from "@/lib/format"

import type { useUpdateAiPermissions } from "./queries"

const STEP = 0.01
const INPUT_STEP = 0.0001
const INPUT_DECIMALS = 4

const thumbClassName =
  "relative block size-3.5 shrink-0 rounded-full border border-ring bg-white ring-ring/50 transition-[color,box-shadow] select-none after:absolute after:-inset-2 hover:ring-3 focus-visible:ring-3 focus-visible:outline-hidden active:ring-3"

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value))
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
  /** Where a marker is drawn on the track. Nothing sets it in this task. */
  danger?: number
  mutate: ReturnType<typeof useUpdateAiPermissions>["mutate"]
}) {
  const [allowValue, setAllowValue] = useState(allowThreshold)
  useEffect(() => setAllowValue(allowThreshold), [allowThreshold])
  const [denyValue, setDenyValue] = useState(denyThreshold)
  useEffect(() => setDenyValue(denyThreshold), [denyThreshold])

  const [allowText, setAllowText] = useState(String(allowThreshold))
  useEffect(() => setAllowText(String(allowThreshold)), [allowThreshold])
  const [denyText, setDenyText] = useState(String(denyThreshold))
  useEffect(() => setDenyText(String(denyThreshold)), [denyThreshold])

  function commitAllow(value: number) {
    mutate(
      { allow_threshold: value },
      {
        onError: (error) => {
          toast.error("Could not change the allow threshold", { description: describeError(error) })
          setAllowValue(allowThreshold)
          setAllowText(String(allowThreshold))
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
          setDenyText(String(denyThreshold))
        },
      },
    )
  }

  function handleSliderChange(value: readonly [number, number]) {
    const [allow, deny] = value
    setAllowValue(allow)
    setAllowText(String(allow))
    setDenyValue(deny)
    setDenyText(String(deny))
  }

  function handleSliderCommit(value: readonly [number, number]) {
    const [allow, deny] = value
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
      commitAllow(roundToInputDecimals(parsed))
    }
  }

  function commitDenyText() {
    const parsed = readTyped(denyText)
    if (parsed !== null) {
      commitDeny(roundToInputDecimals(parsed))
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

  return (
    <div className="flex flex-col gap-3">
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
        <SliderPrimitive.Control className="relative flex w-full touch-none items-center select-none">
          <SliderPrimitive.Track
            data-slot="slider-track"
            className="relative h-1.5 w-full grow overflow-hidden rounded-full select-none"
          >
            <div
              className="pointer-events-none absolute inset-y-0 left-0 bg-status-done"
              style={{ width: `${zoneStart}%` }}
            />
            <div
              className="pointer-events-none absolute inset-y-0 bg-status-warn"
              style={{ left: `${zoneStart}%`, width: `${zoneEnd - zoneStart}%` }}
            />
            <div
              className="pointer-events-none absolute inset-y-0 right-0 bg-status-danger"
              style={{ left: `${zoneEnd}%` }}
            />
            {danger !== undefined ? (
              <div
                role="img"
                aria-label={`Danger ${danger}`}
                className="pointer-events-none absolute inset-y-0 w-0.5 -translate-x-1/2 bg-foreground"
                style={{ left: `${clamp01(danger) * 100}%` }}
              />
            ) : null}
            <SliderPrimitive.Thumb
              index={0}
              aria-label="Allow threshold"
              className={thumbClassName}
            />
            <SliderPrimitive.Thumb
              index={1}
              aria-label="Deny threshold"
              className={thumbClassName}
            />
          </SliderPrimitive.Track>
        </SliderPrimitive.Control>
      </SliderPrimitive.Root>

      <div className="flex justify-between text-xs text-muted-foreground">
        <span>Allow</span>
        <span>Ask</span>
        <span>Deny</span>
      </div>

      <div className="grid grid-cols-2 gap-4">
        <Field>
          <FieldLabel htmlFor="ai-allow-threshold">Allow threshold</FieldLabel>
          <Input
            id="ai-allow-threshold"
            type="number"
            min={0}
            max={1}
            step={INPUT_STEP}
            value={allowText}
            aria-label="Allow threshold"
            onChange={(event) => handleAllowTextChange(event.target.value)}
            onBlur={commitAllowText}
            onKeyDown={blurOnEnter}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="ai-deny-threshold">Deny threshold</FieldLabel>
          <Input
            id="ai-deny-threshold"
            type="number"
            min={0}
            max={1}
            step={INPUT_STEP}
            value={denyText}
            aria-label="Deny threshold"
            onChange={(event) => handleDenyTextChange(event.target.value)}
            onBlur={commitDenyText}
            onKeyDown={blurOnEnter}
          />
        </Field>
      </div>

      <FieldDescription>
        At or under the allow threshold the model allows. At or over the deny threshold it denies.
        Between them, it asks you.
      </FieldDescription>
    </div>
  )
}
