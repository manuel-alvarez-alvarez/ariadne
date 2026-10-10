import { FlagIcon } from "lucide-react"
import { useEffect, useState } from "react"
import type { ParsedWorkflowDto, WorkflowStepDto } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { describeError } from "@/lib/format"

import type { WorkflowParseResult } from "./use-workflow-parse"

function StepBadge({ kind, value }: { kind: "rank" | "gate"; value: string }) {
  const meaning =
    kind === "rank"
      ? "The model rank for this column."
      : "The work state required before this column completes."
  const label = kind === "gate" ? `Gate: ${value}` : value
  return (
    <Tooltip>
      <TooltipTrigger render={<Badge variant="outline" />}>{label}</TooltipTrigger>
      <TooltipContent>{meaning}</TooltipContent>
    </Tooltip>
  )
}

function WorkflowCard({ step }: { step: WorkflowStepDto }) {
  return (
    <article className="min-w-48 flex-1 rounded-lg border bg-card p-3 shadow-sm">
      <div className="flex flex-wrap items-baseline gap-2">
        <h3 className="font-medium">{step.title}</h3>
        <span className="font-mono text-muted-foreground text-xs">{step.id}</span>
      </div>
      <p className="mt-2 max-w-prose whitespace-pre-line text-muted-foreground text-sm">
        {step.description}
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-1.5">
        {step.skills.map((skill) => (
          <Badge key={skill} variant="secondary">
            {skill}
          </Badge>
        ))}
        {step.rank ? <StepBadge kind="rank" value={step.rank} /> : null}
      </div>
    </article>
  )
}

function StepMarker({ index }: { index: number }) {
  return (
    <span className="flex size-7 flex-none items-center justify-center rounded-full border bg-background font-medium text-xs">
      {index + 1}
    </span>
  )
}

function Connector({ gate }: { gate: string | null }) {
  if (!gate) {
    return (
      <div className="flex w-7 flex-none justify-center">
        <span className="h-8 w-px bg-border" aria-hidden />
      </div>
    )
  }
  return (
    <div className="flex w-7 flex-none flex-col items-center gap-1.5 py-1.5">
      <span className="h-3 w-px bg-border" aria-hidden />
      <StepBadge kind="gate" value={gate} />
      <span className="h-3 w-px bg-border" aria-hidden />
    </div>
  )
}

function PipelineStep({ step, index }: { step: WorkflowStepDto; index: number }) {
  return (
    <li>
      <div className="flex gap-3">
        <div className="flex w-7 flex-none flex-col items-center">
          <StepMarker index={index} />
          <span className="w-px flex-1 bg-border" aria-hidden />
        </div>
        <WorkflowCard step={step} />
      </div>
      <Connector gate={step.gate ?? null} />
    </li>
  )
}

export function WorkflowPreview({ parsed }: { parsed: WorkflowParseResult }) {
  const [lastGood, setLastGood] = useState<ParsedWorkflowDto | null>(null)
  useEffect(() => {
    if (parsed.data) setLastGood(parsed.data)
  }, [parsed.data])
  const refusal = parsed.isError ? describeError(parsed.error) : null
  const line = parsed.errorLine
  const steps = (parsed.data ?? lastGood)?.steps ?? []
  return (
    <section aria-label="Workflow preview" className="flex min-h-0 flex-1 flex-col gap-3">
      <header>
        <h2 className="font-medium">Preview</h2>
        <p className="text-muted-foreground text-sm">Columns as the daemon reads this draft.</p>
      </header>
      {refusal ? (
        <p role="alert" className="text-destructive text-sm">
          {line ? `Line ${line}: ` : ""}
          {refusal}
        </p>
      ) : null}
      <ol className={refusal ? "flex flex-col opacity-50" : "flex flex-col"}>
        {steps.map((step, index) => (
          <PipelineStep key={step.id} step={step} index={index} />
        ))}
        {steps.length > 0 ? (
          <li className="flex items-center gap-3">
            <span
              className="flex size-7 flex-none items-center justify-center rounded-full border bg-muted text-muted-foreground"
              aria-hidden
            >
              <FlagIcon className="size-3.5" />
            </span>
            <span className="text-muted-foreground text-sm">End of task</span>
          </li>
        ) : null}
      </ol>
    </section>
  )
}
