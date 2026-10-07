import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useState } from "react"
import { api, qk, unwrap } from "@/api"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import { repositoriesQueryOptions } from "@/features/repositories/queries"

export function AddPullRequestDialog({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const repositories = useQuery(repositoriesQueryOptions())
  const [repo, setRepo] = useState("")
  const [query, setQuery] = useState("")
  const [submitted, setSubmitted] = useState<{ repo: string; q: string } | null>(null)
  const [url, setUrl] = useState("")
  const client = useQueryClient()
  const results = useQuery({
    queryKey: qk.pullRequests.search(submitted?.repo ?? "", submitted?.q ?? ""),
    enabled: open && !!submitted?.repo,
    queryFn: () =>
      unwrap(
        api().GET("/v1/repositories/{id}/pull-requests/search", {
          params: { path: { id: submitted?.repo ?? "" }, query: { q: submitted?.q } },
        }),
      ),
  })
  const add = useMutation({
    mutationFn: (body: { repository_id: string; number: number } | { url: string }) =>
      unwrap(api().POST("/v1/pull-requests", { body })),
    onSuccess: (row) => {
      client.setQueryData(qk.pullRequests.detail(row.id), row)
      void client.invalidateQueries({ queryKey: qk.pullRequests.lists() })
      if (submitted?.repo) void results.refetch()
    },
  })
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Add pull request</DialogTitle>
          <DialogDescription>
            Search an enabled repository or add a request by URL.
          </DialogDescription>
        </DialogHeader>
        <label htmlFor="pr-search-repository">Search repository</label>
        <select
          id="pr-search-repository"
          value={repo}
          onChange={(e) => {
            setRepo(e.target.value)
            setSubmitted(null)
          }}
        >
          <option value="">Select a repository</option>
          {repositories.data
            ?.filter((r) => r.forge?.enabled)
            .map((r) => (
              <option key={r.id} value={r.id}>
                {r.forge?.owner}/{r.forge?.name}
              </option>
            ))}
        </select>
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            setSubmitted({ repo, q: query })
          }}
        >
          <label htmlFor="pr-search">Search requests</label>
          <Input id="pr-search" value={query} onChange={(e) => setQuery(e.target.value)} />
          <Button type="submit" disabled={!repo || results.isFetching}>
            Search
          </Button>
        </form>
        {results.error && <p role="alert">{results.error.message}</p>}
        {results.data?.length === 0 && <p>No matching pull requests.</p>}
        <ul className="max-h-72 space-y-3 overflow-auto">
          {results.data?.map((row) => (
            <li key={row.number} className="flex items-center justify-between gap-4">
              <div>
                <a href={row.url} target="_blank" rel="noreferrer">
                  #{row.number} {row.title}
                </a>
                <p>
                  {row.author_login} · {row.role}
                </p>
              </div>
              <Button
                disabled={row.tracked || add.isPending}
                aria-label={`Add #${row.number}`}
                onClick={() =>
                  add.mutate({ repository_id: submitted?.repo ?? repo, number: row.number })
                }
              >
                {row.tracked ? "Added" : "Add"}
              </Button>
            </li>
          ))}
        </ul>
        <form
          className="flex flex-col gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            add.mutate({ url })
          }}
        >
          <label htmlFor="pr-url">Pull request URL</label>
          <Input id="pr-url" type="url" value={url} onChange={(e) => setUrl(e.target.value)} />
          <Button type="submit" disabled={!url || add.isPending}>
            Add URL
          </Button>
        </form>
        {add.error && <p role="alert">{add.error.message}</p>}
        {add.isSuccess && <p role="status">Pull request added.</p>}
      </DialogContent>
    </Dialog>
  )
}
