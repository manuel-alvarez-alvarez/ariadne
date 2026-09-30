/**
 * One learned row in a side panel of its own, opened from a row of the
 * Learned tab's table.
 *
 * Fetched fresh by id (`GET /v1/permissions/learned/{id}`) rather than handed
 * the row that was clicked, the way the session panel reads its own row again
 * rather than trusting the list's copy: the query key is the one
 * `learned_permission_updated` patches, so an update made elsewhere shows up
 * here without a round trip through the row that opened it.
 *
 * `tool_call`, `options` and `output` are shown pretty-printed, the whole
 * request, the choices it was given and what the model made of it; `output`
 * is `null` wherever the model was never called, which says so instead.
 */

import { useQuery } from "@tanstack/react-query"

import type { LearnedPermissionDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { ErrorState } from "@/components/error-state"
import { Fact, FactList } from "@/components/fact-list"
import { PanelSheet } from "@/components/panel-sheet"
import { StatusBadge } from "@/components/status-badge"
import { SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { When } from "@/components/when"
import { permissionModeLabel } from "@/features/repositories/permission-modes"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { shortId } from "@/lib/format"

import { learnedPermissionQueryOptions } from "./queries"
import { selectedOptionInfo } from "./request-summary"

const OUTCOME_TONE = {
  allow: "bg-status-done-soft text-status-done-fg",
  deny: "bg-status-danger-soft text-status-danger-fg",
} as const

export function LearnedPermissionDetail({ id, onClose }: { id: string; onClose: () => void }) {
  const learned = useQuery(learnedPermissionQueryOptions(id))

  return (
    <PanelSheet onClose={onClose}>
      <SheetContent className="sm:max-w-lg" aria-describedby={undefined}>
        <SheetHeader>
          <SheetTitle>{learned.data ? learned.data.tool_name : `Row ${shortId(id)}`}</SheetTitle>
        </SheetHeader>

        {learned.isPending ? (
          <div className="space-y-3">
            <Skeleton className="h-8 w-48" />
            <Skeleton className="h-40 w-full" />
          </div>
        ) : learned.isError ? (
          <ErrorState
            title={`Could not load row ${shortId(id)}`}
            error={learned.error}
            onRetry={() => void learned.refetch()}
          />
        ) : (
          <LearnedPermissionDetailView learned={learned.data} />
        )}
      </SheetContent>
    </PanelSheet>
  )
}

function LearnedPermissionDetailView({ learned }: { learned: LearnedPermissionDto }) {
  const repositories = useQuery(repositoriesQueryOptions())

  const repositoryPath =
    repositories.data?.find((repository) => repository.id === learned.repository_id)?.path ??
    learned.repository_id
  const option = selectedOptionInfo(learned.options, learned.selected_option)

  return (
    <div className="flex flex-col gap-4">
      <FactList columns={2}>
        <Fact label="ID">
          <CopyableId value={learned.id} label="row id" />
        </Fact>
        <Fact label="Repository">
          <span className="truncate font-mono text-xs" title={repositoryPath}>
            {repositoryPath}
          </span>
        </Fact>
        <Fact label="Repository ID">
          <CopyableId value={learned.repository_id} label="repository id" />
        </Fact>
        <Fact label="Tool">
          <span className="font-mono text-xs">{learned.tool_name}</span>
        </Fact>
        <Fact label="Target">{permissionModeLabel(learned.target)}</Fact>
        <Fact label="Selected option">
          {option.outcome ? (
            <StatusBadge size="sm" label={option.label} tone={OUTCOME_TONE[option.outcome]} />
          ) : (
            option.label
          )}
        </Fact>
        <Fact label="Learned">
          <When at={learned.created_at} label="learned" />
        </Fact>
        <Fact label="Updated">
          <When at={learned.updated_at} label="updated" />
        </Fact>
      </FactList>

      <div className="flex flex-col gap-3">
        <JsonBlock label="Tool call" value={learned.tool_call} />
        <JsonBlock label="Options" value={learned.options} />
        <JsonBlock label="Output" value={learned.output} emptyText="no model output" />
      </div>
    </div>
  )
}

function JsonBlock({
  label,
  value,
  emptyText = "—",
}: {
  label: string
  value: unknown
  emptyText?: string
}) {
  return (
    <div className="space-y-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      <pre className="max-h-64 overflow-auto rounded-md bg-muted p-2 font-mono text-xs">
        {value === null || value === undefined ? emptyText : stringify(value)}
      </pre>
    </div>
  )
}

function stringify(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2) ?? String(value)
  } catch {
    return String(value)
  }
}
