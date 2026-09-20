/**
 * The Impact and path tab (022): what a change to a symbol reaches, and how
 * one symbol reaches another — parity with `ariadne knowledge impact` and
 * `ariadne knowledge path`.
 *
 * Both draw layers left to right on the shared graph. The mode and its inputs
 * are in the URL — `?mode=impact|path&symbol=&depth=&from=&to=` — so a reload
 * or a link shows the same graph. What is typed is written to it on submit,
 * and the depth as soon as it is picked. A click on a node opens the Symbols
 * tab on that symbol.
 */

import { useQuery } from "@tanstack/react-query"
import { type FormEvent, useMemo, useState } from "react"
import { useSearchParams } from "react-router-dom"
import type { KnowledgeImpactDto, KnowledgePathDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { plural } from "@/lib/format"

import type { KnowledgeGraphModel, LegendEntry } from "./graph/graph-model"
import { KnowledgeGraph } from "./graph/knowledge-graph"
import { useLayeredLayout } from "./graph/layered-layout"
import { impactGraph, STOP_CALLERS } from "./impact-graph"
import { pathGraph } from "./path-graph"
import { knowledgeImpactQueryOptions, knowledgePathQueryOptions } from "./queries"
import { SymbolField } from "./suggestion-field"

type Mode = "impact" | "path"

const MODES: readonly { value: Mode; label: string }[] = [
  { value: "impact", label: "Impact" },
  { value: "path", label: "Path" },
]

/** How far each mode walks: the daemon's default first, and its maximum last. */
const DEPTHS: Record<Mode, { fallback: number; max: number }> = {
  impact: { fallback: 2, max: 4 },
  path: { fallback: 6, max: 10 },
}

const IMPACT_LEGEND: LegendEntry[] = [
  { label: "Changed definition", tone: "active" },
  { label: "Caller", tone: "pending" },
  { label: `Walk stopped: over ${STOP_CALLERS} callers`, tone: "danger" },
  { label: "Heuristic call", tone: "pending", dashed: true },
]

const PATH_LEGEND: LegendEntry[] = [
  { label: "From", tone: "active" },
  { label: "Hop", tone: "pending" },
  { label: "To", tone: "done" },
  { label: "Heuristic edge", tone: "ready", dashed: true },
]

/** The depth the URL holds, or the mode's default where it holds none or one out of range. */
function depthOf(value: string | null, mode: Mode): number {
  const { fallback, max } = DEPTHS[mode]
  const depth = Number(value)
  return Number.isInteger(depth) && depth >= 1 && depth <= max ? depth : fallback
}

export function ImpactTab({ repositoryId, gitRef }: { repositoryId: string; gitRef: string }) {
  const [search, setSearch] = useSearchParams()
  const mode: Mode = search.get("mode") === "path" ? "path" : "impact"
  const symbol = search.get("symbol") ?? ""
  const from = search.get("from") ?? ""
  const to = search.get("to") ?? ""
  const depth = depthOf(search.get("depth"), mode)

  /** Writes into the URL, leaving every other param where it was. */
  const write = (changes: Record<string, string | null>) =>
    setSearch(
      (current) => {
        const next = new URLSearchParams(current)
        for (const [key, value] of Object.entries(changes)) {
          if (value === null) next.delete(key)
          else next.set(key, value)
        }
        return next
      },
      { replace: true },
    )
  /** Opens the Symbols tab on a symbol, and takes this tab's own inputs off the URL. */
  const openSymbol = (name: string) =>
    setSearch((current) => {
      const next = new URLSearchParams(current)
      for (const key of ["mode", "depth", "from", "to"]) next.delete(key)
      next.set("tab", "symbols")
      next.set("symbol", name)
      return next
    })

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <fieldset className="flex gap-1">
          <legend className="sr-only">Mode</legend>
          {MODES.map((entry) => (
            <Button
              key={entry.value}
              variant={mode === entry.value ? "secondary" : "outline"}
              size="sm"
              aria-pressed={mode === entry.value}
              onClick={() => write({ mode: entry.value })}
            >
              {entry.label}
            </Button>
          ))}
        </fieldset>
        <QueryForm
          // A new mode or repository starts from what the URL holds.
          key={`${mode}:${repositoryId}`}
          mode={mode}
          repositoryId={repositoryId}
          gitRef={gitRef}
          values={{ symbol, from, to }}
          depth={depth}
          onSubmit={write}
        />
      </div>

      {mode === "impact" ? (
        <ImpactView
          repositoryId={repositoryId}
          gitRef={gitRef}
          symbol={symbol}
          depth={depth}
          onOpen={openSymbol}
        />
      ) : (
        <PathView
          repositoryId={repositoryId}
          gitRef={gitRef}
          from={from}
          to={to}
          depth={depth}
          onOpen={openSymbol}
        />
      )}
    </div>
  )
}

/** The symbol inputs of the mode, and its depth. */
function QueryForm({
  mode,
  repositoryId,
  gitRef,
  values,
  depth,
  onSubmit,
}: {
  mode: Mode
  repositoryId: string
  gitRef: string
  values: { symbol: string; from: string; to: string }
  depth: number
  onSubmit: (changes: Record<string, string | null>) => void
}) {
  const [symbol, setSymbol] = useState(values.symbol)
  const [from, setFrom] = useState(values.from)
  const [to, setTo] = useState(values.to)
  const depths = Array.from({ length: DEPTHS[mode].max }, (_, index) => {
    const value = String(index + 1)
    return { value, label: `Depth ${value}` }
  })

  const submit = (event: FormEvent) => {
    event.preventDefault()
    onSubmit(
      mode === "impact"
        ? { symbol: symbol.trim() || null }
        : { from: from.trim() || null, to: to.trim() || null },
    )
  }

  return (
    <form className="flex flex-wrap items-center gap-2" onSubmit={submit}>
      {mode === "impact" ? (
        <SymbolField
          label="Symbol"
          repositoryId={repositoryId}
          gitRef={gitRef}
          value={symbol}
          onChange={setSymbol}
          className="w-56"
          inputClassName="font-mono"
        />
      ) : (
        <>
          <SymbolField
            label="From symbol"
            repositoryId={repositoryId}
            gitRef={gitRef}
            value={from}
            onChange={setFrom}
            className="w-56"
            inputClassName="font-mono"
          />
          <SymbolField
            label="To symbol"
            repositoryId={repositoryId}
            gitRef={gitRef}
            value={to}
            onChange={setTo}
            className="w-56"
            inputClassName="font-mono"
          />
        </>
      )}
      <Select
        value={String(depth)}
        onValueChange={(value) => value && onSubmit({ depth: value })}
        items={depths}
      >
        <SelectTrigger aria-label="Depth" className="w-28">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {depths.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <Button type="submit" size="sm">
        {mode === "impact" ? "Show impact" : "Find path"}
      </Button>
    </form>
  )
}

function ImpactView({
  repositoryId,
  gitRef,
  symbol,
  depth,
  onOpen,
}: {
  repositoryId: string
  gitRef: string
  symbol: string
  depth: number
  onOpen: (symbol: string) => void
}) {
  const impact = useQuery(knowledgeImpactQueryOptions(repositoryId, gitRef, symbol, depth))

  if (symbol === "") {
    return (
      <EmptyState
        className="py-12"
        title="Name a symbol to see what a change to it reaches."
        description="Its callers show in layers, by how many calls away they are."
      />
    )
  }
  if (impact.isPending) return <Skeleton className="h-96 w-full" />
  if (impact.isError) {
    return (
      <ErrorState
        title="Could not load the impact"
        error={impact.error}
        onRetry={() => void impact.refetch()}
      />
    )
  }
  if (impact.data.length === 0) {
    return (
      <EmptyState
        className="py-12"
        title={`No definition named ${symbol} at ${gitRef}.`}
        description="Search the Symbols tab for the name it is defined under."
      />
    )
  }
  return <ImpactGraph impacts={impact.data} onOpen={onOpen} />
}

function ImpactGraph({
  impacts,
  onOpen,
}: {
  impacts: KnowledgeImpactDto[]
  onOpen: (symbol: string) => void
}) {
  const built = useMemo(() => impactGraph(impacts), [impacts])
  const callers = impacts.reduce((total, impact) => total + impact.callers.length, 0)
  return (
    <LayeredGraph
      model={built.graph}
      symbols={built.symbols}
      legend={IMPACT_LEGEND}
      label="Callers of the changed symbol, in layers by depth"
      summary={callers === 0 ? "Nothing calls it within this depth." : plural(callers, "caller")}
      onOpen={onOpen}
    />
  )
}

function PathView({
  repositoryId,
  gitRef,
  from,
  to,
  depth,
  onOpen,
}: {
  repositoryId: string
  gitRef: string
  from: string
  to: string
  depth: number
  onOpen: (symbol: string) => void
}) {
  const path = useQuery(knowledgePathQueryOptions(repositoryId, gitRef, from, to, depth))

  if (from === "" || to === "") {
    return (
      <EmptyState
        className="py-12"
        title="Name two symbols to find how the first reaches the second."
        description="The shortest path follows calls, references, and imports in the direction they point."
      />
    )
  }
  if (path.isPending) return <Skeleton className="h-96 w-full" />
  if (path.isError) {
    return (
      <ErrorState
        title="Could not load the path"
        error={path.error}
        onRetry={() => void path.refetch()}
      />
    )
  }
  if (path.data.hops.length === 0) {
    return (
      <EmptyState
        className="py-12"
        title={`No path from ${from} to ${to} within depth ${depth}.`}
        description="Try a greater depth, or two other symbols."
      />
    )
  }
  return <PathGraph path={path.data} onOpen={onOpen} />
}

function PathGraph({ path, onOpen }: { path: KnowledgePathDto; onOpen: (symbol: string) => void }) {
  const built = useMemo(() => pathGraph(path), [path])
  return (
    <LayeredGraph
      model={built.graph}
      symbols={built.symbols}
      legend={PATH_LEGEND}
      label="The shortest path between the two symbols"
      summary={plural(path.hops.length - 1, "hop")}
      onOpen={onOpen}
    />
  )
}

/** A model laid out in layers by ELK, drawn once it is. A click on a node opens its symbol. */
function LayeredGraph({
  model,
  symbols,
  legend,
  label,
  summary,
  onOpen,
}: {
  model: KnowledgeGraphModel
  symbols: Map<string, string>
  legend: LegendEntry[]
  label: string
  summary: string
  onOpen: (symbol: string) => void
}) {
  const layout = useLayeredLayout(model)

  if (layout.error) return <ErrorState title="Could not lay out the graph" error={layout.error} />
  if (!layout.graph) return <Skeleton className="h-96 w-full" />
  return (
    <div className="flex flex-col gap-2">
      <p className="text-xs text-muted-foreground">
        {summary}. Click a symbol to open it on the Symbols tab.
      </p>
      <KnowledgeGraph
        className="h-[28rem]"
        graph={layout.graph}
        layout="fixed"
        legend={legend}
        label={label}
        onNodeClick={(node) => {
          const symbol = symbols.get(node)
          if (symbol) onOpen(symbol)
        }}
      />
    </div>
  )
}
