/**
 * Open issues read directly from enabled repository forges (028), the second
 * tab of the Forge screen. A row is the issue, its labels and who it is
 * assigned to, and the goal it can start: the repository is the issue's
 * second line rather than a column of its own.
 *
 * Under many hosts an issue every host lists is one row, matched by its forge
 * repository and its number, with the newest facts among them. Create goal
 * opens on the picked host if it has the repository, else on `local`, else on
 * the first host that has it.
 */
import { useQueries } from "@tanstack/react-query"
import { RefreshCwIcon, TargetIcon } from "lucide-react"
import { useState } from "react"

import {
  api,
  type Host,
  type HostRow,
  type IssueDto,
  LOCAL_HOST,
  qk,
  type RepositoryDto,
  unwrap,
} from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { HostLabel } from "@/components/host-label"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import {
  ForgeFilters,
  forgeName,
  matchesText,
  useNeedsRefresh,
  useSearchFilter,
} from "@/features/forge/forge-filters"
import {
  distinctEnabled,
  filterOption,
  forgeItemKey,
  forgeKey,
  groupForgeItems,
  repositoryOn,
  useHostRepositories,
} from "@/features/forge/forge-hosts"
import { CreateGoalDialog } from "@/features/goals/create-goal-dialog"
import { useHostFilter, usePickedHost } from "@/hooks/use-host-filter"
import { useRowsShowHost } from "@/hooks/use-host-label"

type Row = { host: Host; repository: RepositoryDto; issue: IssueDto }

/**
 * Where Create goal opens for `row`: on `picked` if it has the repository,
 * else on `local`, else on the first host that has it, with that host's id
 * for the repository.
 */
function goalHost(
  known: readonly HostRow<RepositoryDto>[],
  row: Row,
  picked: Host | null,
): { host: Host; repository: string } {
  const key = forgeKey(row.repository)
  const having = known.filter((each) =>
    key === null ? each.host === row.host && each.id === row.repository.id : forgeKey(each) === key,
  )
  const on =
    having.find((each) => each.host === picked) ??
    having.find((each) => each.host === LOCAL_HOST) ??
    having[0]
  return on
    ? { host: on.host, repository: on.id }
    : { host: row.host, repository: row.repository.id }
}

const COLUMNS = [
  { header: "Issue", className: "min-w-72" },
  { header: "Labels" },
  { header: "Assignees" },
  { header: "Updated" },
  { className: "w-32 text-right" },
]

export function IssuesPage() {
  // The hosts the header picker selects, and the repositories each has.
  const hosts = useHostFilter()
  const showHost = useRowsShowHost()
  const repositories = useHostRepositories(hosts)
  const enabled = distinctEnabled(repositories.rows)
  const [repositoryId, setRepositoryId] = useSearchFilter("repo")
  // Assigned to me unless the URL says otherwise.
  const [assignedParam, setAssignedParam] = useSearchFilter("assigned")
  const assignedToMe = assignedParam !== "all"
  const [text, setText] = useSearchFilter("q")
  const picked = usePickedHost()
  const [selected, setSelected] = useState<{ row: Row; host: Host; repository: string } | null>(
    null,
  )
  const [dialogOpen, setDialogOpen] = useState(false)
  // The repository filter is one forge repository, whichever host has it.
  const visible = repositories.rows.filter(
    (repository) =>
      repository.forge?.enabled &&
      (!repositoryId ||
        repositoryOn(repositories.known, repositoryId, repository.host) === repository.id),
  )
  const needsRefresh = useNeedsRefresh(visible)
  const assigned = assignedToMe ? "me" : "all"
  const issueQueries = useQueries({
    queries: visible.map((repository) => ({
      queryKey: qk.issues.list(repository.host, repository.id, assigned),
      queryFn: () =>
        unwrap(
          api(repository.host).GET("/v1/repositories/{id}/issues", {
            params: { path: { id: repository.id }, query: { assigned } },
          }),
        ),
    })),
  })
  const listed: Row[] = issueQueries
    .flatMap((query, index) => {
      const repository = visible[index]
      return repository
        ? (query.data ?? []).map((issue) => ({ host: repository.host, repository, issue }))
        : []
    })
    // Most recently updated first, whichever host it is on.
    .sort((a, b) =>
      a.issue.updated_at < b.issue.updated_at
        ? 1
        : a.issue.updated_at > b.issue.updated_at
          ? -1
          : 0,
    )
    .filter(({ issue }) =>
      matchesText(text, [
        `#${issue.number}`,
        issue.title,
        issue.body,
        ...issue.labels,
        ...issue.assignees,
      ]),
    )
  // One row per issue, whichever hosts list it: the newest first.
  const rows = groupForgeItems(listed, (row) =>
    forgeItemKey(repositories.known, row.host, row.repository.id, row.issue.number),
  )
  const query = {
    data: rows,
    isPending: repositories.isPending || issueQueries.some((result) => result.isPending),
    isError: repositories.errors.length > 0 || issueQueries.some((result) => result.isError),
    error: repositories.errors[0]?.error ?? issueQueries.find((result) => result.error)?.error,
    refetch: () => {
      repositories.refetch()
      issueQueries.forEach((result) => void result.refetch())
    },
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Issues"
        description="Open issues from enabled repositories, read from the forge each time."
        actions={
          // No ledger stands behind this list, as one does behind the pull
          // requests: the issues are read from the forge on every load, so a
          // refresh reads them again. A live webhook tells the screen to on
          // its own (`issues_changed`), so only polling needs the button.
          needsRefresh ? (
            <Button
              variant="outline"
              pending={issueQueries.some((result) => result.isFetching)}
              onClick={() => issueQueries.forEach((result) => void result.refetch())}
            >
              <RefreshCwIcon />
              Refresh
            </Button>
          ) : null
        }
      />
      <ForgeFilters
        repositories={enabled}
        repository={filterOption(enabled, repositories.known, repositoryId)}
        onRepository={setRepositoryId}
        query={text}
        onQuery={setText}
        placeholder="Filter by title or description"
        toggle={{
          id: "assigned-to-me",
          label: "Assigned to me",
          checked: assignedToMe,
          onChange: (checked) => setAssignedParam(checked ? null : "all"),
        }}
      />
      <DataTable
        query={query}
        errorTitle="Could not load issues"
        columns={COLUMNS}
        empty={
          <EmptyState
            emphasis="quiet"
            className="border-0 py-10"
            title={assignedToMe ? "No open issues are assigned to you" : "No open issues"}
            description={
              assignedToMe
                ? "Turn off Assigned to me to see every open issue of the enabled repositories."
                : "Open issues of the repositories with their forge enabled show up here."
            }
          />
        }
        rowKey={(group) => {
          const row = group[0] as Row
          return forgeItemKey(repositories.known, row.host, row.repository.id, row.issue.number)
        }}
        renderRow={(group) => {
          const row = group[0] as Row
          return (
            <TableRow>
              <TableCell className="max-w-md whitespace-normal">
                <div className="flex flex-col gap-0.5">
                  <a
                    href={row.issue.url}
                    target="_blank"
                    rel="noreferrer"
                    className="font-medium underline-offset-3 hover:underline"
                  >
                    #{row.issue.number} {row.issue.title}
                  </a>
                  <span className="text-xs text-muted-foreground">
                    {showHost
                      ? group.map((each) => (
                          <span key={each.host}>
                            <HostLabel host={each.host} />{" "}
                          </span>
                        ))
                      : null}
                    <span className="font-mono">{forgeName(row.repository)}</span>
                  </span>
                </div>
              </TableCell>
              <TableCell className="max-w-64 whitespace-normal">
                {row.issue.labels.length > 0 ? (
                  <span className="flex flex-wrap gap-1">
                    {row.issue.labels.map((label) => (
                      <Badge key={label} variant="outline">
                        {label}
                      </Badge>
                    ))}
                  </span>
                ) : (
                  <span className="text-muted-foreground">-</span>
                )}
              </TableCell>
              <TableCell className="text-muted-foreground">
                {row.issue.assignees.length > 0 ? row.issue.assignees.join(", ") : "-"}
              </TableCell>
              <TableCell className="text-muted-foreground">
                <When at={row.issue.updated_at} format="age" label="updated" />
              </TableCell>
              <TableCell className="text-right">
                <Button
                  size="sm"
                  variant="outline"
                  onClick={() => {
                    setSelected({ row, ...goalHost(repositories.known, row, picked) })
                    setDialogOpen(true)
                  }}
                >
                  <TargetIcon />
                  Create goal
                </Button>
              </TableCell>
            </TableRow>
          )
        }}
      />
      <CreateGoalDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        initial={
          selected
            ? {
                title: selected.row.issue.title,
                description: `${selected.row.issue.body}\n\nIssue: ${selected.row.issue.url}`,
                repository_id: selected.repository,
                issue_url: selected.row.issue.url,
                host: selected.host,
              }
            : undefined
        }
      />
    </div>
  )
}
