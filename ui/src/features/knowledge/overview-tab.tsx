/**
 * The Overview tab (022): one compact card per registered repository, with
 * what its index holds and a Reindex button — parity with `ariadne knowledge
 * status` and `ariadne knowledge reindex`.
 */

import { useQuery } from "@tanstack/react-query"
import { RefreshCwIcon } from "lucide-react"

import type { KnowledgeState, KnowledgeStatusDto, RepositoryDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { StatusBadge } from "@/components/status-badge"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { When } from "@/components/when"
import { folderName, shortSha } from "@/lib/format"

import { knowledgeStatusQueryOptions, useReindexKnowledge } from "./queries"
import { KNOWLEDGE_STATE_META } from "./status"

export function OverviewTab({ repositories }: { repositories: RepositoryDto[] }) {
  return (
    <ul className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
      {repositories.map((repository) => (
        <RepositoryCard key={repository.id} repository={repository} />
      ))}
    </ul>
  )
}

function RepositoryCard({ repository }: { repository: RepositoryDto }) {
  const status = useQuery(knowledgeStatusQueryOptions(repository.id))
  const reindex = useReindexKnowledge(repository.id)
  const name = folderName(repository.path)

  return (
    <li
      aria-label={name}
      className="flex flex-col gap-3 rounded-lg border bg-card p-3 text-card-foreground"
    >
      <div className="flex items-center justify-between gap-2">
        <h2 className="truncate text-sm font-semibold" title={repository.path}>
          {name}
        </h2>
        <div className="flex shrink-0 items-center gap-2">
          {status.data ? <StatusPill state={status.data.state} /> : null}
          <Button
            variant="outline"
            size="sm"
            aria-label={`Reindex ${name}`}
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
        <Skeleton className="h-16 w-full" />
      ) : status.isError ? (
        <ErrorState
          title="Could not load the knowledge status"
          error={status.error}
          onRetry={() => void status.refetch()}
        />
      ) : (
        <StatusBody status={status.data} />
      )}

      {reindex.isError ? (
        <ErrorState title="Could not start a reindex" error={reindex.error} />
      ) : null}
    </li>
  )
}

function StatusPill({ state }: { state: KnowledgeState }) {
  const meta = KNOWLEDGE_STATE_META[state]
  return <StatusBadge label={meta.label} tone={meta.badge} dot={meta.dot} box="badge" />
}

function StatusBody({ status }: { status: KnowledgeStatusDto }) {
  if (status.state === "disabled") {
    return (
      <p className="text-sm text-muted-foreground">
        Knowledge indexing is turned off for this repository.
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-2 text-xs">
      {/* Per ref: a good run of one ref leaves the failure of another, so
          the ref is what says which index a read is refused for. */}
      {status.failures.length > 0 ? (
        <ul aria-label="Failed refs" className="flex flex-col gap-1 text-sm text-destructive">
          {status.failures.map((failure) => (
            <li key={failure.git_ref}>
              <span className="font-mono">{failure.git_ref}</span>: {failure.error}
            </li>
          ))}
        </ul>
      ) : null}

      <dl className="grid grid-cols-2 gap-2">
        <div>
          <dt className="text-muted-foreground">Files</dt>
          <dd className="text-sm font-medium tabular-nums">{status.files}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">Symbols</dt>
          <dd className="text-sm font-medium tabular-nums">{status.symbols}</dd>
        </div>
      </dl>

      {status.languages.length > 0 ? (
        <ul aria-label="Languages" className="flex flex-wrap gap-1">
          {status.languages.map((language) => (
            <li key={language.language}>
              <Badge variant="outline">
                {language.language}
                <span className="text-muted-foreground tabular-nums">{language.files}</span>
              </Badge>
            </li>
          ))}
        </ul>
      ) : null}

      {status.refs.length === 0 ? (
        <p className="text-muted-foreground">No ref indexed yet.</p>
      ) : (
        <ul aria-label="Refs" className="flex flex-col gap-1">
          {status.refs.map((ref) => (
            <li key={ref.git_ref} className="flex flex-wrap items-center gap-2">
              <span className="font-mono">{ref.git_ref}</span>
              <span className="font-mono text-muted-foreground">{shortSha(ref.commit)}</span>
              <When at={ref.indexed_at} label="indexed" />
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
