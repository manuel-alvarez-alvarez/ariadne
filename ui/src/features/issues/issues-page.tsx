/** Open issues read directly from enabled repository forges. */
import { useQueries, useQuery } from "@tanstack/react-query"
import { useState } from "react"

import { api, type IssueDto, qk, type RepositoryDto, unwrap } from "@/api"
import { DataTable } from "@/components/data-table"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { Switch } from "@/components/ui/switch"
import { TableCell, TableRow } from "@/components/ui/table"
import { CreateGoalDialog } from "@/features/goals/create-goal-dialog"
import { repositoriesQueryOptions } from "@/features/repositories/queries"

type Row = { repository: RepositoryDto; issue: IssueDto }

const COLUMNS = [
  { header: "Repository" },
  { header: "Issue" },
  { header: "Labels" },
  { header: "Assignees" },
  { header: "Updated" },
  { className: "text-right" },
]

export function IssuesPage() {
  const repositories = useQuery(repositoriesQueryOptions())
  const enabled = repositories.data?.filter((repository) => repository.forge?.enabled) ?? []
  const [repositoryId, setRepositoryId] = useState("")
  const [assignedToMe, setAssignedToMe] = useState(true)
  const [selected, setSelected] = useState<Row | null>(null)
  const [dialogOpen, setDialogOpen] = useState(false)
  const visible = repositoryId
    ? enabled.filter((repository) => repository.id === repositoryId)
    : enabled
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
  const rows: Row[] = issueQueries.flatMap((query, index) => {
    const repository = visible[index]
    return repository ? (query.data ?? []).map((issue) => ({ repository, issue })) : []
  })
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
      <PageHeader title="Issues" description="Open issues from enabled repositories." />
      <div className="flex items-center gap-4">
        <label htmlFor="issue-repository">Repository</label>
        <select
          id="issue-repository"
          value={repositoryId}
          onChange={(event) => setRepositoryId(event.target.value)}
        >
          <option value="">All enabled repositories</option>
          {enabled.map((repository) => (
            <option key={repository.id} value={repository.id}>
              {repository.forge?.owner}/{repository.forge?.name}
            </option>
          ))}
        </select>
        <Switch id="assigned-to-me" checked={assignedToMe} onCheckedChange={setAssignedToMe} />
        <label htmlFor="assigned-to-me">Assigned to me</label>
      </div>
      <DataTable
        query={query}
        errorTitle="Could not load issues"
        columns={COLUMNS}
        empty={<p>No open issues.</p>}
        rowKey={(row) => `${row.repository.id}-${row.issue.number}`}
        renderRow={(row) => (
          <TableRow>
            <TableCell>
              {row.repository.forge?.owner}/{row.repository.forge?.name}
            </TableCell>
            <TableCell>
              <a href={row.issue.url} target="_blank" rel="noreferrer">
                #{row.issue.number} {row.issue.title}
              </a>
            </TableCell>
            <TableCell>{row.issue.labels.join(", ")}</TableCell>
            <TableCell>{row.issue.assignees.join(", ")}</TableCell>
            <TableCell>{row.issue.updated_at}</TableCell>
            <TableCell className="text-right">
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  setSelected(row)
                  setDialogOpen(true)
                }}
              >
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
