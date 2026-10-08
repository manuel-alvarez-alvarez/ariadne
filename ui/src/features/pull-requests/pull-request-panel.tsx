import { useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"
import { Link, useLocation, useSearchParams } from "react-router-dom"
import type { PullRequestDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { PanelSheet } from "@/components/panel-sheet"
import { Button } from "@/components/ui/button"
import { PaneBody, PaneHeader, PaneTitle } from "@/components/ui/docked-pane"
import { sessionPanelFrom } from "@/routes/paths"
import { pullRequestQueryOptions, useRemovePullRequest } from "./queries"

export function PullRequestPanel({ id, onClose }: { id: string; onClose: () => void }) {
  const query = useQuery(pullRequestQueryOptions(id))
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
                  <FieldValue pull={query.data} field={key} value={value} />
                </dd>
              </div>
            ))}
          </dl>
        )}
      </PaneBody>
    </PanelSheet>
  )
}

/**
 * One field as `ariadne pr inspect` prints it, with the three that point
 * somewhere made into links: the request on the forge, each failed check's
 * run, and the request's session, opened as its own panel in this one's
 * place.
 */
function FieldValue({
  pull,
  field,
  value,
}: {
  pull: PullRequestDto
  field: string
  value: unknown
}): ReactNode {
  const [search] = useSearchParams()
  const { pathname } = useLocation()
  if (field === "url") {
    return (
      <a href={pull.url} target="_blank" rel="noreferrer">
        {pull.url}
      </a>
    )
  }
  if (field === "session_id" && pull.session_id) {
    return <Link to={sessionPanelFrom(pathname, search, pull.session_id)}>{pull.session_id}</Link>
  }
  if (field === "failed_checks") {
    const checks = pull.failed_checks ?? []
    if (!checks.length) return "none"
    return (
      <ul className="flex flex-col gap-1">
        {checks.map((check) => (
          <li key={`${check.name}:${check.url}`}>
            {check.url ? (
              <a href={check.url} target="_blank" rel="noreferrer">
                {check.name}
              </a>
            ) : (
              check.name
            )}{" "}
            <span className="text-muted-foreground">({check.conclusion})</span>
          </li>
        ))}
      </ul>
    )
  }
  return value === null ? "null" : String(value)
}
