import { useQuery } from "@tanstack/react-query"
import { useEffect, useState } from "react"

import { api, type ParsedWorkflowDto, unwrap, type WorkflowStepDto } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { describeError } from "@/lib/format"

function StepBadge({ kind, value }: { kind: "rank" | "gate"; value: string }) {
  const meaning =
    kind === "rank"
      ? "The model rank for this column."
      : "The work state required before this column completes."
  return (
    <Tooltip>
      <TooltipTrigger render={<Badge variant="outline" />}>{value}</TooltipTrigger>
      <TooltipContent>{meaning}</TooltipContent>
    </Tooltip>
  )
}

function WorkflowCard({ step }: { step: WorkflowStepDto }) {
  return (
    <article className="min-w-48 rounded-lg border bg-card p-3 shadow-sm">
      <h3 className="font-medium">{step.title}</h3>
      <p className="mt-0.5 font-mono text-muted-foreground text-xs">{step.id}</p>
      <p className="mt-3 whitespace-pre-line text-sm">{step.description}</p>
      <div className="mt-3 flex flex-wrap gap-1.5">
        {step.skills.map((skill) => (
          <Badge key={skill} variant="secondary">
            {skill}
          </Badge>
        ))}
        {step.rank ? <StepBadge kind="rank" value={step.rank} /> : null}
        {step.gate ? <StepBadge kind="gate" value={step.gate} /> : null}
      </div>
    </article>
  )
}

export function WorkflowPreview({ document }: { document: string }) {
  const [settledDocument, setSettledDocument] = useState(document)
  const [lastGood, setLastGood] = useState<ParsedWorkflowDto | null>(null)
  useEffect(() => {
    const timeout = window.setTimeout(() => setSettledDocument(document), 250)
    return () => window.clearTimeout(timeout)
  }, [document])
  const parsed = useQuery({
    queryKey: ["workflows", "parse", settledDocument],
    queryFn: () =>
      unwrap(api().POST("/v1/workflows/parse", { body: { document: settledDocument } })),
  })
  useEffect(() => {
    if (parsed.data) setLastGood(parsed.data)
  }, [parsed.data])
  const refusal = parsed.isError ? describeError(parsed.error) : null
  const line =
    parsed.isError &&
    typeof (parsed.error as { details?: { line?: unknown } }).details?.line === "number"
      ? (parsed.error as unknown as { details: { line: number } }).details.line
      : null
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
      <div
        className={refusal ? "flex gap-3 overflow-x-auto opacity-50" : "flex gap-3 overflow-x-auto"}
      >
        {(parsed.data ?? lastGood)?.steps.map((step) => (
          <WorkflowCard key={step.id} step={step} />
        ))}
      </div>
    </section>
  )
}
