/**
 * Search and explore one symbol's neighbourhood (022). The URL owns the
 * current name; this tab owns the definition choice and the browsing stack.
 */

import { useQuery } from "@tanstack/react-query"
import { ArrowLeftIcon, ArrowRightIcon, SearchIcon } from "lucide-react"
import { type FormEvent, type ReactNode, useEffect, useMemo, useState } from "react"

import type { KnowledgeHitDto, KnowledgeSymbolDto, RepositoryDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"

import type { LegendEntry } from "./graph/graph-model"
import { KnowledgeGraph } from "./graph/knowledge-graph"
import { knowledgeSearchQueryOptions, knowledgeSymbolQueryOptions } from "./queries"
import { PathField, SymbolField } from "./suggestion-field"
import {
  SYMBOL_RELATION_META,
  SYMBOL_RELATIONS,
  type SymbolTarget,
  symbolsGraph,
} from "./symbols-graph"

const SYMBOL_KINDS = [
  "function",
  "method",
  "class",
  "module",
  "interface",
  "type",
  "macro",
  "constant",
  "test",
  "heading",
] as const

const KIND_OPTIONS = [
  { value: "all", label: "Any kind" },
  ...SYMBOL_KINDS.map((kind) => ({
    value: kind,
    label: `${kind[0]?.toUpperCase()}${kind.slice(1)}`,
  })),
]

const LEGEND: LegendEntry[] = [
  ...SYMBOL_RELATIONS.map((relation) => SYMBOL_RELATION_META[relation]),
  { label: "Heuristic", tone: "pending", dashed: true },
  { label: "↗ Other repository", tone: "pending" },
]

interface SymbolLocation {
  name: string
  repositoryId: string
  gitRef: string
  definitionKey: string | null
}

interface SearchFilters {
  q: string
  kind?: string
  path?: string
}

function locationKey(repositoryId: string, path: string, line: number): string {
  return `${repositoryId}:${path}:${line}`
}

function definitionKey(symbol: KnowledgeSymbolDto): string {
  return locationKey(symbol.repository_id, symbol.path, symbol.start_line)
}

function numberedSource(source: string, startLine: number) {
  let number = startLine
  return source.split("\n").map((text) => ({ number: number++, text }))
}

function sameLocation(a: SymbolLocation | undefined, b: SymbolLocation): boolean {
  return a?.name === b.name && a.repositoryId === b.repositoryId && a.gitRef === b.gitRef
}

export function SymbolsTab({
  repositories,
  repositoryId,
  gitRef,
  symbolName,
  onNavigateSymbol,
}: {
  repositories: RepositoryDto[]
  repositoryId: string
  gitRef: string
  symbolName: string
  onNavigateSymbol: (name: string, repositoryId: string, gitRef: string) => void
}) {
  const initial = symbolName
    ? [{ name: symbolName, repositoryId, gitRef, definitionKey: null }]
    : []
  const [history, setHistory] = useState<{ entries: SymbolLocation[]; index: number }>({
    entries: initial,
    index: initial.length - 1,
  })
  const [preferredDefinition, setPreferredDefinition] = useState<string | null>(null)

  useEffect(() => {
    if (!symbolName) return
    const incoming = { name: symbolName, repositoryId, gitRef, definitionKey: null }
    setHistory((current) => {
      if (sameLocation(current.entries[current.index], incoming)) return current
      return {
        entries: [...current.entries.slice(0, current.index + 1), incoming],
        index: current.index + 1,
      }
    })
    setPreferredDefinition(null)
  }, [symbolName, repositoryId, gitRef])

  const visit = (target: SymbolTarget | KnowledgeHitDto) => {
    const targetRepositoryId =
      "repository_id" in target ? target.repository_id : target.repositoryId
    const targetRepository = repositories.find((repository) => repository.id === targetRepositoryId)
    const next: SymbolLocation = {
      name: target.name,
      repositoryId: targetRepositoryId,
      gitRef:
        targetRepositoryId === repositoryId ? gitRef : (targetRepository?.base_branch ?? gitRef),
      definitionKey: locationKey(targetRepositoryId, target.path, target.line),
    }
    setHistory((current) => ({
      entries: [...current.entries.slice(0, current.index + 1), next],
      index: current.index + 1,
    }))
    setPreferredDefinition(next.definitionKey)
    onNavigateSymbol(next.name, next.repositoryId, next.gitRef)
  }

  const move = (index: number) => {
    const entry = history.entries[index]
    if (!entry) return
    setHistory((current) => ({ ...current, index }))
    setPreferredDefinition(entry.definitionKey)
    onNavigateSymbol(entry.name, entry.repositoryId, entry.gitRef)
  }

  return (
    <div className="flex flex-col gap-4">
      <SymbolSearch repositoryId={repositoryId} gitRef={gitRef} onPick={visit} />
      {symbolName ? (
        <SymbolNeighbourhood
          repositoryId={repositoryId}
          gitRef={gitRef}
          name={symbolName}
          preferredDefinition={preferredDefinition}
          onPickDefinition={(key) => {
            setPreferredDefinition(key)
            setHistory((current) => {
              const entries = [...current.entries]
              const active = entries[current.index]
              if (active) entries[current.index] = { ...active, definitionKey: key }
              return { ...current, entries }
            })
          }}
          onPickNode={visit}
          history={
            <div className="flex gap-1">
              <Button
                variant="outline"
                size="icon-sm"
                aria-label="Back symbol"
                disabled={history.index <= 0}
                onClick={() => move(history.index - 1)}
              >
                <ArrowLeftIcon />
              </Button>
              <Button
                variant="outline"
                size="icon-sm"
                aria-label="Forward symbol"
                disabled={history.index >= history.entries.length - 1}
                onClick={() => move(history.index + 1)}
              >
                <ArrowRightIcon />
              </Button>
            </div>
          }
        />
      ) : (
        <EmptyState
          emphasis="quiet"
          className="py-12"
          title="Find a symbol to explore."
          description="Search by name, then open a result to see its callers and related definitions."
        />
      )}
    </div>
  )
}

function SymbolSearch({
  repositoryId,
  gitRef,
  onPick,
}: {
  repositoryId: string
  gitRef: string
  onPick: (hit: KnowledgeHitDto) => void
}) {
  const [query, setQuery] = useState("")
  const [kind, setKind] = useState("")
  const [path, setPath] = useState("")
  const [filters, setFilters] = useState<SearchFilters>({ q: "" })
  const results = useQuery(knowledgeSearchQueryOptions(repositoryId, gitRef, filters))

  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const q = query.trim()
    if (!q) return
    setFilters({ q, ...(kind ? { kind } : {}), ...(path.trim() ? { path: path.trim() } : {}) })
  }

  return (
    <div className="flex flex-col gap-2">
      <form className="flex flex-wrap items-end gap-2" onSubmit={submit}>
        <Label className="min-w-56 flex-1 flex-col items-start gap-1">
          Search symbols
          <SymbolField
            label="Search symbols"
            placeholder="Name or identifier"
            repositoryId={repositoryId}
            gitRef={gitRef}
            value={query}
            onChange={setQuery}
            className="w-full"
          />
        </Label>
        <Label className="w-44 flex-col items-start gap-1">
          Kind
          <Select
            value={kind || "all"}
            onValueChange={(value) => setKind(value === "all" || !value ? "" : value)}
            items={KIND_OPTIONS}
          >
            <SelectTrigger aria-label="Kind" className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {KIND_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Label>
        <Label className="min-w-48 flex-1 flex-col items-start gap-1">
          Path
          <PathField
            label="Path"
            placeholder="src/"
            repositoryId={repositoryId}
            gitRef={gitRef}
            value={path}
            onChange={setPath}
            className="w-full"
          />
        </Label>
        <Button type="submit" size="sm" disabled={!query.trim()}>
          <SearchIcon />
          Search
        </Button>
      </form>
      {results.isError ? (
        <ErrorState
          title="Could not search symbols"
          error={results.error}
          onRetry={() => void results.refetch()}
        />
      ) : null}
      {results.isFetching ? <Skeleton className="h-20 w-full" /> : null}
      {filters.q && results.data ? (
        results.data.length > 0 ? (
          <ul aria-label="Symbol search results" className="divide-y rounded-lg border">
            {results.data.map((hit) => (
              <li key={locationKey(hit.repository_id, hit.path, hit.line)}>
                <button
                  type="button"
                  className="flex w-full items-center gap-3 px-3 py-2 text-left hover:bg-muted/50"
                  onClick={() => onPick(hit)}
                >
                  <span className="min-w-0 flex-1 truncate font-medium">{hit.name}</span>
                  <Badge variant="outline">{hit.kind}</Badge>
                  <span className="font-mono text-xs text-muted-foreground">
                    {hit.path}:{hit.line}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted-foreground">No symbols match this search.</p>
        )
      ) : null}
    </div>
  )
}

function SymbolNeighbourhood({
  repositoryId,
  gitRef,
  name,
  preferredDefinition,
  onPickDefinition,
  onPickNode,
  history,
}: {
  repositoryId: string
  gitRef: string
  name: string
  preferredDefinition: string | null
  onPickDefinition: (key: string) => void
  onPickNode: (target: SymbolTarget) => void
  history: ReactNode
}) {
  const context = useQuery(knowledgeSymbolQueryOptions(repositoryId, gitRef, name, "context"))
  const source = useQuery(knowledgeSymbolQueryOptions(repositoryId, gitRef, name, "source"))
  const selected =
    context.data?.find((definition) => definitionKey(definition) === preferredDefinition) ??
    context.data?.[0]
  const selectedKey = selected ? definitionKey(selected) : ""
  const sourceDefinition = source.data?.find(
    (definition) => definitionKey(definition) === selectedKey,
  )
  const built = useMemo(() => (selected ? symbolsGraph(selected) : null), [selected])

  if (context.isPending) return <Skeleton className="h-[32rem] w-full" />
  if (context.isError) {
    return (
      <ErrorState
        title="Could not load the symbol"
        error={context.error}
        onRetry={() => void context.refetch()}
      />
    )
  }
  if (!selected || !built) {
    return <EmptyState emphasis="quiet" className="py-12" title="This symbol has no definition." />
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        {history}
        {context.data.length > 1 ? (
          <Select value={selectedKey} onValueChange={(value) => value && onPickDefinition(value)}>
            <SelectTrigger aria-label="Definition" className="min-w-64">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {context.data.map((definition) => {
                const key = definitionKey(definition)
                return (
                  <SelectItem key={key} value={key}>
                    {definition.path}:{definition.start_line}
                  </SelectItem>
                )
              })}
            </SelectContent>
          </Select>
        ) : null}
      </div>
      <div className="flex flex-col gap-3 xl:flex-row">
        <KnowledgeGraph
          className="h-[32rem] min-w-0 flex-1"
          graph={built.graph}
          layout="circle"
          legend={LEGEND}
          label={`Neighbourhood of ${selected.name}`}
          onNodeClick={(node) => {
            const target = built.targets.get(node)
            if (target) onPickNode(target)
          }}
        />
        <SymbolDetails
          symbol={selected}
          source={sourceDefinition?.source}
          pending={source.isPending}
        />
      </div>
    </div>
  )
}

function SymbolDetails({
  symbol,
  source,
  pending,
}: {
  symbol: KnowledgeSymbolDto
  source: string | null | undefined
  pending: boolean
}) {
  return (
    <aside
      aria-label="Symbol details"
      className="flex max-h-[32rem] flex-col gap-3 overflow-auto rounded-lg border p-3 xl:w-[30rem]"
    >
      <div>
        <h2 className="break-all font-mono text-sm font-semibold">{symbol.signature}</h2>
        <p className="mt-1 font-mono text-xs text-muted-foreground">
          {symbol.path}:{symbol.start_line}-{symbol.end_line}
        </p>
      </div>
      {symbol.doc ? <p className="whitespace-pre-wrap text-sm">{symbol.doc}</p> : null}
      {pending ? (
        <Skeleton className="h-40 w-full" />
      ) : source ? (
        <section
          aria-label="Source"
          className="overflow-auto rounded-md bg-muted p-3 font-mono text-xs leading-5"
        >
          <pre>
            <code>
              {numberedSource(source, symbol.start_line).map((line) => (
                <span key={line.number} className="grid grid-cols-[3rem_1fr]">
                  <span className="select-none pr-3 text-right text-muted-foreground">
                    {line.number}{" "}
                  </span>
                  <span>{line.text || " "}</span>
                </span>
              ))}
            </code>
          </pre>
        </section>
      ) : (
        <p className="text-sm text-muted-foreground">No source is available.</p>
      )}
    </aside>
  )
}
