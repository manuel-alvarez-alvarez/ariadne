/**
 * A pending permission as a card above the composer: the call it asks about
 * and its options. Up, Down and 1 to 9 select an option, and Enter or a click
 * answers it. The picker is the one the console of today uses.
 */

import { ShieldIcon } from "lucide-react"
import type { KeyboardEvent } from "react"

import type { StepDto } from "@/api"

import { PermissionPicker } from "../console/blocks"
import type { PermissionOption } from "../console/transcript"

type PermissionStep = Extract<StepDto, { type: "permission" }>

export function PermissionCard({
  step,
  selected,
  answering,
  onSelect,
  onAnswer,
  onKeyDown,
}: {
  step: PermissionStep
  selected: number
  answering: string | null
  onSelect: (index: number) => void
  onAnswer: (option: PermissionOption) => void
  onKeyDown: (event: KeyboardEvent) => boolean
}) {
  const options = step.options.map((option) => ({
    id: option.id,
    name: option.name,
    kind: option.kind ?? undefined,
  }))
  return (
    <section
      aria-label="Pending permission"
      data-step-id={step.id}
      className="rounded-lg border bg-card px-3 py-2 text-xs text-card-foreground shadow-xs"
    >
      <div className="flex items-center gap-1.5 font-semibold">
        <ShieldIcon aria-hidden className="size-3.5 shrink-0 text-status-warn-fg" />
        Permission · {step.tool.title}
        {step.subagent ? (
          <span className="font-normal text-muted-foreground">
            · asked by subagent {step.subagent.title}
          </span>
        ) : null}
      </div>
      {step.tool.subject ? (
        <code className="mt-1.5 block whitespace-pre-wrap break-words rounded-md bg-muted px-2 py-1 font-mono">
          {step.tool.subject}
        </code>
      ) : null}
      {step.question && step.question !== step.tool.title ? (
        <p className="mt-1.5 break-words">{step.question}</p>
      ) : null}
      {step.ai ? <p className="mt-1 font-mono text-muted-foreground">{step.ai}</p> : null}
      <PermissionPicker
        options={options}
        selected={selected}
        answering={answering}
        onSelect={onSelect}
        onAnswer={onAnswer}
        onKeyDown={onKeyDown}
      />
    </section>
  )
}
