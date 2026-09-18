/**
 * One repository's knowledge base (022): its indexing status, a Reindex
 * button, a search box over what it indexed, and the interactions found
 * between files — parity with `ariadne knowledge status|reindex|search
 * |interactions` (014's parity rule, spec 015).
 *
 * Reached from a row on the repositories screen and from the command
 * palette, the way the memory page (019) is reached from its row: a
 * knowledge base belongs to one repository, same as a memory does, so there
 * is no list beside it worth keeping on screen.
 */

import { useQuery } from "@tanstack/react-query"
import { RefreshCwIcon } from "lucide-react"
import { useState } from "react"
import { Link } from "react-router-dom"

import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Fact, FactList } from "@/components/fact-list"
import { PageHeader } from "@/components/page-header"
import { StatusBadge } from "@/components/status-badge"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { When } from "@/components/when"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { folderName, shortSha } from "@/lib/format"
import { paths } from "@/routes/paths"

import {
  knowledgeInteractionsQueryOptions,
  knowledgeSearchQueryOptions,
  knowledgeStatusQueryOptions,
  useReindexKnowledge,
} from "./queries"
import { CONFIDENCE_LABELS, INTERACTION_KIND_LABELS, KNOWLEDGE_STATE_META } from "./status"
import type {
  KnowledgeEndpointDto,
  KnowledgeInteractionGroupDto,
  KnowledgeSearchResultDto,
  KnowledgeState,
  KnowledgeStatusDto,
} from "./types"

export function KnowledgePage({ repositoryId }: { repositoryId: string }) {
  const repositories = useQuery(repositoriesQueryOptions())
  const repository = repositories.data?.find((row) => row.id === repositoryId)

  if (repositories.isPending) return <LoadingKnowledge />

  if (repositories.isError) {
    return (
      <ErrorState
        title="Could not load the repository"
        error={repositories.error}
        onRetry={() => void repositories.refetch()}
        showIcon
      />
    )
  }

  if (!repository) {
    return (
      <EmptyState
        emphasis="quiet"
        className="my-12"
        title="No repository by that id."
        description="It may have been removed since the link was made."
        action={
          <Button variant="outline" size="sm" render={<Link to={paths.repositories()} />}>
            Back to repositories
          </Button>
        }
      />
    )
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Knowledge"
        description={`What ${folderName(repository.path)} has indexed — its status, a search over it, and the interactions found between files.`}
        actions={
          <Button variant="outline" size="sm" render={<Link to={paths.repositories()} />}>
            Back to repositories
          </Button>
        }
      />

      <KnowledgeStatusCard repositoryId={repositoryId} />
      <KnowledgeSearch repositoryId={repositoryId} />
      <KnowledgeInteractions repositoryId={repositoryId} />
    </div>
  )
}

function LoadingKnowledge() {
  return (
    <div className="flex flex-col gap-4">
      <Skeleton className="h-8 w-48" />
      {[0, 1, 2].map((row) => (
        <Skeleton key={row} className="h-24 w-full" />
      ))}
    </div>
  )
}

// ── Status ──────────────────────────────────────────────────────────────

function KnowledgeStatusCard({ repositoryId }: { repositoryId: string }) {
  const status = useQuery(knowledgeStatusQueryOptions(repositoryId))
  const reindex = useReindexKnowledge(repositoryId)

  return (
    <div className="flex flex-col gap-3 rounded-lg border bg-card p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="text-sm font-semibold">Status</h2>
        <div className="flex items-center gap-2">
          {status.data ? <StatusPill state={status.data.state} /> : null}
          <Button
            variant="outline"
            size="sm"
            // Only a *known* `disabled` state rules it out; a status that
            // has not loaded yet — still pending, or failed to load at all —
            // must not itself block a reindex, or a transient read failure
            // would leave a user unable to fix the very thing that failed.
            disabled={status.data?.state === "disabled" || reindex.isPending}
            onClick={() => reindex.mutate()}
          >
            <RefreshCwIcon />
            Reindex
          </Button>
        </div>
      </div>

      {status.isPending ? (
        <Skeleton className="h-20 w-full" />
      ) : status.isError ? (
        <ErrorState
          title="Could not load the knowledge status"
          error={status.error}
          onRetry={() => void status.refetch()}
        />
      ) : status.data ? (
        <KnowledgeStatusBody status={status.data} />
      ) : null}

      {reindex.isError ? (
        <ErrorState title="Could not start a reindex" error={reindex.error} />
      ) : null}
    </div>
  )
}

function StatusPill({ state }: { state: KnowledgeState }) {
  const meta = KNOWLEDGE_STATE_META[state]
  return <StatusBadge label={meta.label} tone={meta.badge} dot={meta.dot} box="badge" />
}

function KnowledgeStatusBody({ status }: { status: KnowledgeStatusDto }) {
  if (status.state === "disabled") {
    return (
      <p className="text-sm text-muted-foreground">
        Knowledge indexing is turned off for this repository.
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-3">
      {status.state === "failed" && status.error ? (
        <p className="text-sm text-destructive">{status.error}</p>
      ) : null}

      <FactList columns={3}>
        <Fact label="Files">{status.files}</Fact>
        <Fact label="Symbols">{status.symbols}</Fact>
        <Fact label="Refs">
          {status.refs.length === 0 ? (
            <span className="text-xs text-muted-foreground">none indexed yet</span>
          ) : (
            <ul className="flex flex-col gap-1 text-xs">
              {status.refs.map((ref) => (
                <li key={ref.git_ref} className="flex flex-wrap items-center gap-2">
                  <span className="font-mono">{ref.git_ref}</span>
                  <span className="font-mono text-muted-foreground">{shortSha(ref.commit)}</span>
                  <When at={ref.indexed_at} label="indexed" />
                </li>
              ))}
            </ul>
          )}
        </Fact>
      </FactList>

      {status.languages.length > 0 ? (
        <Table className="rounded-md border">
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead>Language</TableHead>
              <TableHead className="text-right">Files</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {status.languages.map((language) => (
              <TableRow key={language.language} className="hover:bg-transparent">
                <TableCell className="text-xs">{language.language}</TableCell>
                <TableCell className="text-right text-xs">{language.files}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      ) : null}
    </div>
  )
}

// ── Search ────────────────────────────────────────────────────────────────

const SEARCH_COLUMNS = [
  { header: "Location" },
  { header: "Kind" },
  { header: "Name" },
  { header: "Signature", className: "min-w-48" },
]

function KnowledgeSearch({ repositoryId }: { repositoryId: string }) {
  const [q, setQ] = useState("")
  const [kind, setKind] = useState("")
  const [path, setPath] = useState("")
  const results = useQuery(knowledgeSearchQueryOptions(repositoryId, { q, kind, path }))
  const filtering = q.trim() || kind.trim() || path.trim()

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold">Search</h2>
      <div className="flex flex-wrap gap-2">
        <Input
          value={q}
          placeholder="Search the knowledge base"
          aria-label="Search knowledge"
          autoComplete="off"
          className="min-w-48 flex-1"
          onChange={(event) => setQ(event.target.value)}
        />
        <Input
          value={kind}
          placeholder="Kind"
          aria-label="Filter by kind"
          autoComplete="off"
          className="w-32"
          onChange={(event) => setKind(event.target.value)}
        />
        <Input
          value={path}
          placeholder="Path"
          aria-label="Filter by path"
          autoComplete="off"
          className="w-48"
          onChange={(event) => setPath(event.target.value)}
        />
      </div>

      {filtering ? (
        <DataTable
          query={results}
          errorTitle="Could not search the knowledge base"
          columns={SEARCH_COLUMNS}
          empty={<EmptyState className="border-0 py-8" emphasis="quiet" title="No matches." />}
          rowKey={(result) => `${result.path}:${result.line}:${result.name}`}
          renderRow={(result) => <SearchRow result={result} />}
        />
      ) : (
        <p className="text-sm text-muted-foreground">
          Type a query, or a kind or path filter, to search.
        </p>
      )}
    </div>
  )
}

function SearchRow({ result }: { result: KnowledgeSearchResultDto }) {
  return (
    <TableRow>
      <TableCell className="font-mono text-xs">
        {result.path}:{result.line}
      </TableCell>
      <TableCell>
        <Badge variant="outline">{result.kind}</Badge>
      </TableCell>
      <TableCell className="text-xs">{result.name}</TableCell>
      <TableCell className="min-w-48 whitespace-normal font-mono text-xs text-muted-foreground">
        {result.signature ?? "—"}
      </TableCell>
    </TableRow>
  )
}

// ── Interactions ────────────────────────────────────────────────────────

function KnowledgeInteractions({ repositoryId }: { repositoryId: string }) {
  const interactions = useQuery(knowledgeInteractionsQueryOptions(repositoryId))

  return (
    <div className="flex flex-col gap-3">
      <h2 className="text-sm font-semibold">Interactions</h2>
      {interactions.isPending ? (
        <Skeleton className="h-24 w-full" />
      ) : interactions.isError ? (
        <ErrorState
          title="Could not load interactions"
          error={interactions.error}
          onRetry={() => void interactions.refetch()}
        />
      ) : !interactions.data || interactions.data.length === 0 ? (
        <EmptyState className="py-8" emphasis="quiet" title="No interactions found yet." />
      ) : (
        <div className="flex flex-col gap-3">
          {interactions.data.map((group) => (
            <InteractionGroup key={group.kind} group={group} />
          ))}
        </div>
      )}
    </div>
  )
}

function InteractionGroup({ group }: { group: KnowledgeInteractionGroupDto }) {
  return (
    <div className="rounded-lg border bg-card p-3">
      <h3 className="mb-2 text-xs font-medium text-muted-foreground uppercase">
        {INTERACTION_KIND_LABELS[group.kind]}
      </h3>
      <ul className="flex flex-col gap-2">
        {group.edges.map((edge, index) => (
          // The daemon gives an edge no id of its own; its position in an
          // otherwise-static list is the only key there is.
          // biome-ignore lint/suspicious/noArrayIndexKey: edges carry no id of their own
          <li key={index} className="flex flex-wrap items-center gap-2 text-xs">
            <EndpointLabel endpoint={edge.from} />
            <span className="text-muted-foreground" aria-hidden>
              →
            </span>
            <EndpointLabel endpoint={edge.to} />
            <Badge variant={edge.confidence === "exact" ? "secondary" : "outline"}>
              {CONFIDENCE_LABELS[edge.confidence]}
            </Badge>
          </li>
        ))}
      </ul>
    </div>
  )
}

function EndpointLabel({ endpoint }: { endpoint: KnowledgeEndpointDto }) {
  return (
    <span className="font-mono">
      {endpoint.repository_id}:{endpoint.path}:{endpoint.line} {endpoint.symbol}
    </span>
  )
}
