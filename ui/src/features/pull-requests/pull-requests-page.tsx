import { useMutation, useQuery } from "@tanstack/react-query"
import { useState } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"
import { api, qk, unwrap } from "@/api"
import { DataTable } from "@/components/data-table"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import { TableCell, TableRow } from "@/components/ui/table"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { paths } from "@/routes/paths"
import { AddPullRequestDialog } from "./add-pull-request-dialog"
import { useRemovePullRequest } from "./queries"

const COLUMNS = [
  "Repository",
  "Request",
  "Role",
  "Tracked by",
  "Draft",
  "Checks",
  "Review decision",
  "Unanswered comments",
  "Updated",
  "Actions",
].map((header) => ({ header }))
export function PullRequestsPage() {
  const [search, setSearch] = useSearchParams()
  const navigate = useNavigate()
  const [adding, setAdding] = useState(false)
  const filters = {
    repo: search.get("repo") || undefined,
    role: search.get("role") || undefined,
    state: search.get("state") || undefined,
  }
  const rows = useQuery({
    queryKey: qk.pullRequests.list(filters),
    queryFn: () => unwrap(api().GET("/v1/pull-requests", { params: { query: filters } })),
  })
  const repositories = useQuery(repositoriesQueryOptions())
  const remove = useRemovePullRequest()
  const refresh = useMutation({
    mutationFn: () =>
      unwrap(
        api().POST("/v1/pull-requests/refresh", { params: { query: { repo: filters.repo } } }),
      ),
  })
  function filter(key: string, value: string) {
    const next = new URLSearchParams(search)
    if (value) next.set(key, value)
    else next.delete(key)
    setSearch(next)
  }
  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Pull requests"
        description="Your authored, requested, and manually tracked pull requests."
        actions={
          <>
            <Button variant="outline" disabled={refresh.isPending} onClick={() => refresh.mutate()}>
              Refresh
            </Button>
            <Button onClick={() => setAdding(true)}>Add pull request</Button>
          </>
        }
      />
      <div className="flex flex-wrap items-center gap-3">
        <label htmlFor="pr-repository">Repository</label>
        <select
          id="pr-repository"
          value={filters.repo ?? ""}
          onChange={(e) => filter("repo", e.target.value)}
        >
          <option value="">All repositories</option>
          {repositories.data?.map((r) => (
            <option key={r.id} value={r.id}>
              {r.forge ? `${r.forge.owner}/${r.forge.name}` : r.path}
            </option>
          ))}
        </select>
        <label htmlFor="pr-role">Role</label>
        <select
          id="pr-role"
          value={filters.role ?? ""}
          onChange={(e) => filter("role", e.target.value)}
        >
          <option value="">All roles</option>
          <option value="author">Author</option>
          <option value="reviewer">Reviewer</option>
        </select>
        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={filters.state === "all"}
            onChange={(e) => filter("state", e.target.checked ? "all" : "")}
          />
          Include closed and merged
        </label>
      </div>
      {(refresh.error || remove.error) && (
        <p role="alert">{refresh.error?.message ?? remove.error?.message}</p>
      )}
      <DataTable
        query={rows}
        errorTitle="Could not load pull requests"
        columns={COLUMNS}
        empty={<p>No pull requests.</p>}
        rowKey={(row) => row.id}
        renderRow={(row) => {
          const repo = repositories.data?.find((r) => r.id === row.repository_id)
          const inspect = () => navigate(paths.pullRequest(row.id, search))
          return (
            <TableRow onClick={inspect} className="cursor-pointer">
              <TableCell>
                {repo?.forge ? `${repo.forge.owner}/${repo.forge.name}` : row.repository_id}
              </TableCell>
              <TableCell>
                <a
                  href={row.url}
                  target="_blank"
                  rel="noreferrer"
                  onClick={(e) => e.stopPropagation()}
                >
                  #{row.number} {row.title}
                </a>
              </TableCell>
              <TableCell>{row.role}</TableCell>
              <TableCell>{row.tracked_by}</TableCell>
              <TableCell>{String(row.draft)}</TableCell>
              <TableCell>{row.checks}</TableCell>
              <TableCell>{row.review_decision}</TableCell>
              <TableCell>{row.unanswered_comments}</TableCell>
              <TableCell>{row.updated_at}</TableCell>
              <TableCell onClick={(e) => e.stopPropagation()}>
                <Button variant="ghost" aria-label={`Inspect #${row.number}`} onClick={inspect}>
                  Inspect
                </Button>
                {row.tracked_by === "user" && (
                  <Button
                    variant="ghost"
                    disabled={remove.isPending}
                    onClick={() => remove.mutate(row.id)}
                  >
                    Remove
                  </Button>
                )}
              </TableCell>
            </TableRow>
          )
        }}
      />
      <AddPullRequestDialog open={adding} onOpenChange={setAdding} />
    </div>
  )
}
