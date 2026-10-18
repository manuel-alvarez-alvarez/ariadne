/**
 * Open issues read directly from enabled repository forges (028), the second
 * tab of the Forge screen. A row is the issue, its labels and who it is
 * assigned to, and the goal it can start: the repository is the issue's
 * second line rather than a column of its own.
 */
import { useQueries, useQuery } from "@tanstack/react-query"
import { RefreshCwIcon, TargetIcon } from "lucide-react"
import { useState } from "react"

import { api, type IssueDto, qk, type RepositoryDto, unwrap } from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
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
import { CreateGoalDialog } from "@/features/goals/create-goal-dialog"
import { repositoriesQueryOptions } from "@/features/repositories/queries"

type Row = { repository: RepositoryDto; issue: IssueDto }

const COLUMNS = [
  { header: "Issue", className: "min-w-72" },
  { header: "Labels" },
  { header: "Assignees" },
  { header: "Updated" },
  { className: "w-32 text-right" },
]

export function IssuesPage() {
  const repositories = useQuery(repositoriesQueryOptions())
  const enabled = repositories.data?.filter((repository) => repository.forge?.enabled) ?? []
  const [repositoryId, setRepositoryId] = useSearchFilter("repo")
  // Assigned to me unless the URL says otherwise.
  const [assignedParam, setAssignedParam] = useSearchFilter("assigned")
  const assignedToMe = assignedParam !== "all"
  const [text, setText] = useSearchFilter("q")
  const [selected, setSelected] = useState<Row | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const visible = repositoryId
    ? enabled.filter((repository) => repository.id === repositoryId)
    : enabled
  const needsRefresh = useNeedsRefresh(visible)
  const assigned = assignedToMe ? "me" : "all"
  const issueQueries = useQueries({
    queries: visible.map((repository) => ({
      queryKey: qk.issues.list(repository.id, assigned),
      queryFn: () =>
        unwrap(
          api().GET("/v1/repositories/{id}/issues", {
            params: { path: { id: repository.id }, query: { assigned } },
          }),
        ),
    })),
  })
  const rows: Row[] = issueQueries
    .flatMap((query, index) => {
      const repository = visible[index]
      return repository ? (query.data ?? []).map((issue) => ({ repository, issue })) : []
    })
    .filter(({ issue }) =>
      matchesText(text, [
        `#${issue.number}`,
        issue.title,
        issue.body,
        ...issue.labels,
        ...issue.assignees,
      ]),
    )
  const query = {
    data: rows,
    isPending: repositories.isPending || issueQueries.some((result) => result.isPending),
    isError: repositories.isError || issueQueries.some((result) => result.isError),
    error: repositories.error ?? issueQueries.find((result) => result.error)?.error,
    refetch: () => {
      void repositories.refetch()
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
        repository={repositoryId}
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
        rowKey={(row) => `${row.repository.id}-${row.issue.number}`}
        renderRow={(row) => (
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
                <span className="font-mono text-xs text-muted-foreground">
                  {forgeName(row.repository)}
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
                  setSelected(row)
                  setDialogOpen(true)
                }}
              >
                <TargetIcon />
                Create goal
              </Button>
            </TableCell>
          </TableRow>
        )}
      />
      <CreateGoalDialog
        open={dialogOpen}
        onOpenChange={setDialogOpen}
        initial={
          selected
            ? {
                title: selected.issue.title,
                description: `${selected.issue.body}\n\nIssue: ${selected.issue.url}`,
                repository_id: selected.repository.id,
                issue_url: selected.issue.url,
              }
            : undefined
        }
      />
    </div>
  )
}
