/**
 * The Repositories tab (022): how the registered repositories use each
 * other, as a graph — parity with `ariadne knowledge interactions`, for every
 * repository at once.
 *
 * The picked repository is read at the picked ref; every other one at its
 * own base, which is what the daemon reads without a ref. A click on an edge
 * lists the file-level edges under it; a click on a node picks that
 * repository.
 */

import { useQueries } from "@tanstack/react-query"
import { useMemo, useState } from "react"

import type { KnowledgeEdgeDto, KnowledgeEndpointDto, RepositoryDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { folderName, plural } from "@/lib/format"

import type { LegendEntry } from "./graph/graph-model"
import { KnowledgeGraph } from "./graph/knowledge-graph"
import { knowledgeInteractionsQueryOptions } from "./queries"
import { type InteractionFilters, repositoriesGraph } from "./repositories-graph"
import {
  CONFIDENCE_LABELS,
  confidenceLabel,
  INTERACTION_KIND_TONES,
  INTERACTION_KINDS,
  interactionKindLabel,
  stepLabel,
} from "./status"

const CONFIDENCE_OPTIONS = [
  { value: "all", label: "Any confidence" },
  { value: "exact", label: CONFIDENCE_LABELS.exact },
  { value: "heuristic", label: CONFIDENCE_LABELS.heuristic },
] as const

const LEGEND: LegendEntry[] = [
  ...INTERACTION_KINDS.map((kind) => ({
    label: interactionKindLabel(kind),
    tone: INTERACTION_KIND_TONES[kind],
  })),
  { label: "Heuristic only", tone: "pending", dashed: true },
]

export function RepositoriesTab({
  repositories,
  repositoryId,
  gitRef,
  onPickRepository,
}: {
  repositories: RepositoryDto[]
  repositoryId: string
  gitRef: string
  onPickRepository: (repositoryId: string) => void
}) {
  const answers = useQueries({
    queries: repositories.map((repository) =>
      knowledgeInteractionsQueryOptions(
        repository.id,
        repository.id === repositoryId ? gitRef : undefined,
      ),
    ),
  })
  const [filters, setFilters] = useState<InteractionFilters>({
    kinds: new Set(INTERACTION_KINDS),
    confidence: "all",
  })
  const [selected, setSelected] = useState<string | null>(null)

  const pending = answers.some((answer) => answer.isPending)
  const failed = answers.find((answer) => answer.isError)
  const data = answers.map((answer) => answer.data ?? [])
  // A new array every render; its contents are what the graph depends on.
  const dataKey = answers.map((answer) => answer.dataUpdatedAt).join()
  // biome-ignore lint/correctness/useExhaustiveDependencies: `data` changes with `dataKey`
  const built = useMemo(
    () => repositoriesGraph(repositories, data, filters),
    [repositories, dataKey, filters],
  )

  if (pending) return <Skeleton className="h-96 w-full" />
  if (failed?.error) {
    return (
      <ErrorState
        title="Could not load the interactions"
        error={failed.error}
        onRetry={() => void failed.refetch()}
      />
    )
  }
  if (data.every((groups) => groups.length === 0)) {
    return (
      <EmptyState
        className="py-12"
        title="No interactions between repositories yet."
        description="An edge shows once one indexed repository depends on, calls, or references another."
      />
    )
  }

  const group = selected ? built.groups.get(selected) : undefined
  const names = new Map(repositories.map((row) => [row.id, folderName(row.path)]))

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        {INTERACTION_KINDS.map((kind) => {
          const on = filters.kinds.has(kind)
          return (
            <Button
              key={kind}
              variant={on ? "secondary" : "outline"}
              size="sm"
              aria-pressed={on}
              onClick={() => {
                const kinds = new Set(filters.kinds)
                if (on) kinds.delete(kind)
                else kinds.add(kind)
                setFilters({ ...filters, kinds })
              }}
            >
              {interactionKindLabel(kind)}
            </Button>
          )
        })}
        <Select
          value={filters.confidence}
          onValueChange={(value) =>
            setFilters({
              ...filters,
              confidence: (value ?? "all") as InteractionFilters["confidence"],
            })
          }
          items={CONFIDENCE_OPTIONS}
        >
          <SelectTrigger aria-label="Filter by confidence" className="w-40">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {CONFIDENCE_OPTIONS.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="flex flex-col gap-3 lg:flex-row">
        <KnowledgeGraph
          className="h-[28rem] flex-1"
          graph={built.graph}
          layout="force"
          legend={LEGEND}
          label="Interactions between repositories"
          onNodeClick={onPickRepository}
          onEdgeClick={setSelected}
        />
        <aside
          aria-label="Edge ends"
          className="flex max-h-[28rem] flex-col gap-2 overflow-y-auto rounded-lg border p-3 lg:w-96"
        >
          {group ? (
            <>
              <h2 className="text-sm font-semibold">
                {interactionKindLabel(group.kind)}: {names.get(group.from)} → {names.get(group.to)}
              </h2>
              <p className="text-xs text-muted-foreground">{plural(group.edges.length, "edge")}</p>
              <ul className="flex flex-col gap-2">
                {group.edges.map((edge) => (
                  // An edge has no id; the whole of it is what makes it one.
                  <EdgeEnds key={JSON.stringify(edge)} edge={edge} />
                ))}
              </ul>
            </>
          ) : (
            <p className="text-sm text-muted-foreground">
              Click an edge to list the files at its ends.
            </p>
          )}
        </aside>
      </div>
    </div>
  )
}

function EdgeEnds({ edge }: { edge: KnowledgeEdgeDto }) {
  return (
    <li className="flex flex-col gap-1 rounded-md border p-2 text-xs">
      <End end={edge.from} />
      <span className="text-muted-foreground" aria-hidden>
        ↓
      </span>
      <End end={edge.to} />
      <span className="flex items-center gap-2">
        <Badge variant={edge.confidence === "exact" ? "secondary" : "outline"}>
          {confidenceLabel(edge.confidence)}
        </Badge>
        {/* What the confidence rests on: the step that joined the ends. */}
        <span className="text-muted-foreground">via {stepLabel(edge.step)}</span>
      </span>
    </li>
  )
}

function End({ end }: { end: KnowledgeEndpointDto }) {
  return (
    <span className="flex flex-wrap gap-x-2 break-all">
      <span className="font-mono">
        {end.path}:{end.line}
      </span>
      <span className="font-mono text-muted-foreground">{end.symbol}</span>
    </span>
  )
}
