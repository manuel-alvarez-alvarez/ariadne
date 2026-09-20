/**
 * The Files tab (022): which file of one repository ref uses which, as a
 * graph — parity with `ariadne knowledge graph`.
 *
 * The graph is drawn at file level or, merged, at directory level. Path
 * text, edge kinds and a switch for unlinked files filter it without a
 * rebuild. A click on a file opens its outline and its edges beside the
 * graph, with a link per symbol to the Symbols tab.
 *
 * Everything the user picks lives in the URL, beside the screen's own
 * params: `?level=`, `?depth=`, `?open=`, `?filter=`, `?hidden_kinds=`,
 * `?isolated=hide`, `?limit=` and `?file=`.
 */

import { useQuery } from "@tanstack/react-query"
import { useDeferredValue, useMemo } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { KnowledgeGraphDto, KnowledgeGraphEdgeDto } from "@/api"
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
import { plural } from "@/lib/format"

import { type FilesLevel, filesGraph, filesVisibility } from "./files-graph"
import { KnowledgeGraph } from "./graph/knowledge-graph"
import { knowledgeGraphQueryOptions, knowledgeOutlineQueryOptions } from "./queries"
import { confidenceLabel } from "./status"
import { PathField } from "./suggestion-field"

/** What the daemon keeps without a limit, and the most it keeps with one. */
const DEFAULT_LIMIT = 2000
const MAX_LIMIT = 10_000

const DEPTH_OPTIONS = [1, 2, 3, 4].map((depth) => ({
  value: String(depth),
  label: plural(depth, "segment"),
}))

/** The params this tab owns: the screen's own are left alone. */
const PARAMS = [
  "level",
  "depth",
  "open",
  "filter",
  "hidden_kinds",
  "isolated",
  "limit",
  "file",
] as const
type Param = (typeof PARAMS)[number]

function readLimit(value: string | null): number | undefined {
  const limit = Number(value)
  return Number.isInteger(limit) && limit > 0 ? Math.min(limit, MAX_LIMIT) : undefined
}

export function FilesTab({ repositoryId, gitRef }: { repositoryId: string; gitRef: string }) {
  const [search, setSearch] = useSearchParams()
  const level: FilesLevel = search.get("level") === "file" ? "file" : "directory"
  const depth = Math.max(1, Number(search.get("depth")) || 2)
  const open = level === "directory" ? search.get("open") : null
  const text = search.get("filter") ?? ""
  const hiddenKindsParam = search.get("hidden_kinds") ?? ""
  const hiddenKinds = useMemo(
    () => new Set(hiddenKindsParam.split(",").filter(Boolean)),
    [hiddenKindsParam],
  )
  const hideIsolated = search.get("isolated") === "hide"
  const limit = readLimit(search.get("limit"))
  const file = search.get("file")

  /** Writes this tab's picks into the URL, leaving every other param where it was. */
  const pick = (changes: Partial<Record<Param, string | null>>) =>
    setSearch(
      (current) => {
        const next = new URLSearchParams(current)
        for (const [key, value] of Object.entries(changes)) {
          if (value === null || value === "") next.delete(key)
          else if (value !== undefined) next.set(key, value)
        }
        return next
      },
      { replace: true },
    )

  const answer = useQuery(knowledgeGraphQueryOptions(repositoryId, gitRef, limit))
  const built = useMemo(
    () => (answer.data ? filesGraph(answer.data, level, depth, open) : null),
    [answer.data, level, depth, open],
  )
  // Typing keeps its pace on a big graph: the hiding follows a beat behind.
  const deferredText = useDeferredValue(text)
  const hidden = useMemo(
    () =>
      built ? filesVisibility(built, { text: deferredText, hiddenKinds, hideIsolated }) : undefined,
    [built, deferredText, hiddenKinds, hideIsolated],
  )

  if (answer.isPending) return <Skeleton className="h-96 w-full" />
  if (answer.isError) {
    return (
      <ErrorState
        title="Could not load the file graph"
        error={answer.error}
        onRetry={() => void answer.refetch()}
      />
    )
  }
  const data = answer.data
  if (!built || data.nodes.length === 0) {
    return (
      <EmptyState
        className="py-12"
        title="No files indexed at this ref yet."
        description="The graph shows once the knowledge base has indexed this ref."
      />
    )
  }

  const toggleKind = (kind: string) => {
    const kinds = new Set(hiddenKinds)
    if (kinds.has(kind)) kinds.delete(kind)
    else kinds.add(kind)
    pick({ hidden_kinds: [...kinds].sort().join(",") })
  }
  const shownLimit = limit ?? DEFAULT_LIMIT
  const selected = file && data.nodes.some((node) => node.path === file) ? file : null
  const shownNodes = built.graph.order - (hidden?.nodes.size ?? 0)
  const shownEdges = built.graph.size - (hidden?.edges.size ?? 0)

  const onNodeClick = (node: string) => {
    if (level === "file") {
      pick({ file: node })
      return
    }
    if (node === open) {
      pick({ open: null, file: null })
      return
    }
    if (open && data.nodes.some((file) => file.path === node)) {
      pick({ file: node })
      return
    }
    pick({ open: node, file: null })
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <fieldset aria-label="Level" className="flex gap-1">
          {(["file", "directory"] as const).map((value) => (
            <Button
              key={value}
              variant={level === value ? "secondary" : "outline"}
              size="sm"
              aria-pressed={level === value}
              onClick={() =>
                pick(
                  value === "file"
                    ? { level: "file", open: null, file: null }
                    : { level: "directory", file: null },
                )
              }
            >
              {value === "file" ? "Files" : "Directories"}
            </Button>
          ))}
        </fieldset>
        {level === "directory" ? (
          <Select
            value={String(depth)}
            onValueChange={(value) =>
              pick({ depth: value === "2" ? null : value, open: null, file: null })
            }
            items={DEPTH_OPTIONS}
          >
            <SelectTrigger aria-label="Directory depth" className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {DEPTH_OPTIONS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : null}
        {open ? (
          <Button variant="outline" size="sm" onClick={() => pick({ open: null, file: null })}>
            Collapse
          </Button>
        ) : null}
        <PathField
          label="Filter by path"
          repositoryId={repositoryId}
          gitRef={gitRef}
          limit={limit}
          value={text}
          onChange={(value) => pick({ filter: value })}
          className="w-56"
        />
        {built.kinds.map((kind) => {
          const on = !hiddenKinds.has(kind)
          return (
            <Button
              key={kind}
              variant={on ? "secondary" : "outline"}
              size="sm"
              aria-pressed={on}
              onClick={() => toggleKind(kind)}
            >
              {kind}
            </Button>
          )
        })}
        <Button
          variant={hideIsolated ? "secondary" : "outline"}
          size="sm"
          aria-pressed={hideIsolated}
          onClick={() => pick({ isolated: hideIsolated ? null : "hide" })}
        >
          Hide unlinked
        </Button>
      </div>

      {data.truncated ? (
        <div className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
          <span>
            Showing {data.nodes.length} of {data.total_nodes} files.
          </span>
          {shownLimit < MAX_LIMIT ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => pick({ limit: String(Math.min(shownLimit * 2, MAX_LIMIT)) })}
            >
              Show up to {Math.min(shownLimit * 2, MAX_LIMIT)}
            </Button>
          ) : null}
        </div>
      ) : null}

      <p className="text-sm text-muted-foreground">
        Showing {shownNodes} of {plural(built.graph.order, "node")} and {shownEdges} of{" "}
        {plural(built.graph.size, "edge")}.
      </p>

      <div className="flex flex-col gap-3 lg:flex-row">
        <KnowledgeGraph
          className="h-[32rem] flex-1"
          graph={built.graph}
          layout="force"
          legend={built.legend}
          label={
            level === "file" ? "Dependencies between files" : "Dependencies between directories"
          }
          hidden={hidden}
          onNodeClick={onNodeClick}
        />
        {selected ? (
          <FilePane
            repositoryId={repositoryId}
            gitRef={gitRef}
            path={selected}
            data={data}
            search={search}
            onPick={(path) => pick({ file: path })}
            onClose={() => pick({ file: null })}
          />
        ) : null}
      </div>
    </div>
  )
}

/** One file's outline and its edges both ways, beside the graph. */
function FilePane({
  repositoryId,
  gitRef,
  path,
  data,
  search,
  onPick,
  onClose,
}: {
  repositoryId: string
  gitRef: string
  path: string
  data: KnowledgeGraphDto
  search: URLSearchParams
  onPick: (path: string) => void
  onClose: () => void
}) {
  const outline = useQuery(knowledgeOutlineQueryOptions(repositoryId, gitRef, path))
  const incoming = data.edges.filter((edge) => edge.to === path)
  const outgoing = data.edges.filter((edge) => edge.from === path)

  /** The Symbols tab on one name, at the same repository and ref. */
  const symbolLink = (name: string) => {
    const next = new URLSearchParams(search)
    for (const key of PARAMS) next.delete(key)
    next.set("tab", "symbols")
    next.set("symbol", name)
    return { search: `?${next}` }
  }

  return (
    <aside
      aria-label="File"
      className="flex max-h-[32rem] flex-col gap-3 overflow-y-auto rounded-lg border p-3 lg:w-96"
    >
      <div className="flex items-start justify-between gap-2">
        <h2 className="font-mono text-sm font-semibold break-all">{path}</h2>
        <Button variant="ghost" size="sm" onClick={onClose}>
          Close
        </Button>
      </div>

      <section aria-label="Outline" className="flex flex-col gap-1">
        <h3 className="text-xs font-medium text-muted-foreground">Outline</h3>
        {outline.isPending ? (
          <Skeleton className="h-16 w-full" />
        ) : outline.isError ? (
          <ErrorState
            title="Could not load the outline"
            error={outline.error}
            onRetry={() => void outline.refetch()}
          />
        ) : outline.data.length === 0 ? (
          <p className="text-xs text-muted-foreground">No definitions in this file.</p>
        ) : (
          <ul className="flex flex-col gap-1 text-xs">
            {outline.data.map((entry) => (
              <li key={`${entry.start_line}:${entry.name}`} className="flex flex-wrap gap-x-2">
                <Link
                  to={symbolLink(entry.name)}
                  className="font-mono underline-offset-2 hover:underline"
                >
                  {entry.name}
                </Link>
                <span className="text-muted-foreground">
                  {entry.kind} · {entry.start_line}–{entry.end_line}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <EdgeList title="Incoming" edges={incoming} end={(edge) => edge.from} onPick={onPick} />
      <EdgeList title="Outgoing" edges={outgoing} end={(edge) => edge.to} onPick={onPick} />
    </aside>
  )
}

function EdgeList({
  title,
  edges,
  end,
  onPick,
}: {
  title: string
  edges: KnowledgeGraphEdgeDto[]
  /** The file at the far end of an edge. */
  end: (edge: KnowledgeGraphEdgeDto) => string
  onPick: (path: string) => void
}) {
  return (
    <section aria-label={title} className="flex flex-col gap-1">
      <h3 className="text-xs font-medium text-muted-foreground">
        {title} · {plural(edges.length, "edge")}
      </h3>
      <ul className="flex flex-col gap-1 text-xs">
        {edges.map((edge) => (
          <li
            key={JSON.stringify([edge.from, edge.to, edge.kind])}
            className="flex flex-wrap items-center gap-2"
          >
            <button
              type="button"
              className="font-mono break-all underline-offset-2 hover:underline"
              onClick={() => onPick(end(edge))}
            >
              {end(edge)}
            </button>
            <span className="text-muted-foreground">
              {edge.kind} × {edge.count}
            </span>
            <Badge variant={edge.confidence === "exact" ? "secondary" : "outline"}>
              {confidenceLabel(edge.confidence)}
            </Badge>
          </li>
        ))}
      </ul>
    </section>
  )
}
