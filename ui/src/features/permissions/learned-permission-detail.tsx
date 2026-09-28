/**
 * One learned approval in a side panel of its own, opened from a row of the
 * Learned tab's table.
 *
 * Fetched fresh by id (`GET /v1/permissions/learned/{id}`) rather than handed
 * the row that was clicked, the way the session panel reads its own row again
 * rather than trusting the list's copy: the query key is the one
 * `learned_permission_updated` patches, so an edit made from another window
 * shows up here without a round trip through the row that opened it.
 *
 * `tool_call` and `options` are shown pretty-printed for a console row, the
 * request an agent actually made; a manual row recorded none, and says so
 * instead. Links out to the session and the task go through the same
 * `sessionPanelFrom` / `taskPanelFrom` helpers every other panel link in the
 * app uses, so the filter and the tab this panel opened over stay behind it.
 */

import { useQuery } from "@tanstack/react-query"
import { Link, useLocation, useSearchParams } from "react-router-dom"

import type { LearnedPermissionDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { ErrorState } from "@/components/error-state"
import { Fact, FactList } from "@/components/fact-list"
import { PanelSheet } from "@/components/panel-sheet"
import { SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { When } from "@/components/when"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { shortId } from "@/lib/format"
import { sessionPanelFrom, taskPanelFrom } from "@/routes/paths"

import { learnedPermissionQueryOptions } from "./queries"

const SOURCE_LABELS = { console: "Console", manual: "Manual" } as const
const AI_LABELS = { allow: "Allow", ask: "Ask", deny: "Deny" } as const

export function LearnedPermissionDetail({ id, onClose }: { id: string; onClose: () => void }) {
  const learned = useQuery(learnedPermissionQueryOptions(id))

  return (
    <PanelSheet onClose={onClose}>
      <SheetContent className="sm:max-w-lg" aria-describedby={undefined}>
        <SheetHeader>
          <SheetTitle>
            {learned.data ? learned.data.tool_name : `Approval ${shortId(id)}`}
          </SheetTitle>
        </SheetHeader>

        {learned.isPending ? (
          <div className="space-y-3">
            <Skeleton className="h-8 w-48" />
            <Skeleton className="h-40 w-full" />
          </div>
        ) : learned.isError ? (
          <ErrorState
            title={`Could not load approval ${shortId(id)}`}
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
  const { pathname } = useLocation()
  const [search] = useSearchParams()

  const repositoryPath =
    repositories.data?.find((repository) => repository.id === learned.repository_id)?.path ??
    learned.repository_id

  return (
    <div className="flex flex-col gap-4">
      <FactList columns={2}>
        <Fact label="ID">
          <CopyableId value={learned.id} label="approval id" />
        </Fact>
        <Fact label="Repository">
          <span className="truncate font-mono text-xs">{repositoryPath}</span>
        </Fact>
        <Fact label="Repository ID">
          <CopyableId value={learned.repository_id} label="repository id" />
        </Fact>
        <Fact label="Tool">
          <span className="font-mono text-xs">{learned.tool_name}</span>
        </Fact>
        <Fact label="Kind">{learned.kind}</Fact>
        <Fact label="Source">{SOURCE_LABELS[learned.source]}</Fact>
        <Fact label="Selected option">{learned.selected_option ?? "—"}</Fact>
        <Fact label="Session">
          {learned.session_id ? (
            <Link
              to={sessionPanelFrom(pathname, search, learned.session_id)}
              className="font-mono text-xs underline-offset-3 hover:underline"
            >
              session {shortId(learned.session_id)}
            </Link>
          ) : (
            "—"
          )}
        </Fact>
        <Fact label="Task">
          {learned.task_id ? (
            <Link
              to={taskPanelFrom(pathname, search, learned.task_id)}
              className="font-mono text-xs underline-offset-3 hover:underline"
            >
              task {shortId(learned.task_id)}
            </Link>
          ) : (
            "—"
          )}
        </Fact>
        <Fact label="AI label">{learned.label ? AI_LABELS[learned.label] : "—"}</Fact>
        <Fact label="Danger">{learned.danger != null ? learned.danger.toFixed(2) : "—"}</Fact>
        <Fact label="Allow threshold">
          {learned.allow_threshold != null ? learned.allow_threshold.toFixed(2) : "—"}
        </Fact>
        <Fact label="Deny threshold">
          {learned.deny_threshold != null ? learned.deny_threshold.toFixed(2) : "—"}
        </Fact>
        <Fact label="Learned">
          <When at={learned.created_at} label="learned" />
        </Fact>
        <Fact label="Updated">
          <When at={learned.updated_at} label="updated" />
        </Fact>
      </FactList>

      {learned.source === "manual" ? (
        <p className="rounded-lg border border-dashed p-3 text-sm text-muted-foreground">
          Added by hand — no request was recorded for it.
        </p>
      ) : (
        <div className="flex flex-col gap-3">
          <RequestBlock label="Tool call" value={learned.tool_call} />
          <RequestBlock label="Options" value={learned.options} />
        </div>
      )}
    </div>
  )
}

function RequestBlock({ label, value }: { label: string; value: unknown }) {
  return (
    <div className="space-y-1">
      <p className="text-xs text-muted-foreground">{label}</p>
      <pre className="max-h-64 overflow-auto rounded-md bg-muted p-2 font-mono text-xs">
        {value === null || value === undefined ? "—" : stringify(value)}
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
