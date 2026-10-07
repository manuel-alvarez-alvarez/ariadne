import { useQuery } from "@tanstack/react-query"
import { api, type PullRequestDto, qk, unwrap } from "@/api"
import { ErrorState } from "@/components/error-state"
import { PanelSheet } from "@/components/panel-sheet"
import { Button } from "@/components/ui/button"
import { PaneBody, PaneHeader, PaneTitle } from "@/components/ui/docked-pane"
import { useRemovePullRequest } from "./queries"

export function PullRequestPanel({ id, onClose }: { id: string; onClose: () => void }) {
  const query = useQuery<PullRequestDto>({
    queryKey: qk.pullRequests.detail(id),
    queryFn: () => unwrap(api().GET("/v1/pull-requests/{id}", { params: { path: { id } } })),
  })
  const remove = useRemovePullRequest(onClose)
  return (
    <PanelSheet onClose={onClose}>
      <PaneHeader>
        <PaneTitle>{query.data?.title ?? "Pull request"}</PaneTitle>
        {query.data?.tracked_by === "user" && (
          <Button variant="outline" disabled={remove.isPending} onClick={() => remove.mutate(id)}>
            Remove
          </Button>
        )}
        {remove.error && <p role="alert">{remove.error.message}</p>}
      </PaneHeader>
      <PaneBody>
        {query.isPending ? (
          <p>Loading pull request…</p>
        ) : query.isError ? (
          <ErrorState
            title="Could not load pull request"
            error={query.error}
            onRetry={() => void query.refetch()}
          />
        ) : (
          <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-3 break-all">
            {Object.entries(query.data).map(([key, value]) => (
              <div key={key} className="contents">
                <dt className="text-muted-foreground">{key}</dt>
                <dd>
                  {key === "url" ? (
                    <a href={String(value)} target="_blank" rel="noreferrer">
                      {String(value)}
                    </a>
                  ) : value === null ? (
                    "null"
                  ) : (
                    String(value)
                  )}
                </dd>
              </div>
            ))}
          </dl>
        )}
      </PaneBody>
    </PanelSheet>
  )
}
