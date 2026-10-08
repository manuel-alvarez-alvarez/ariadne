/**
 * The open pull requests of the enabled repositories (026, 029), the first
 * tab of the Forge screen: every one, the user's own, or the ones that ask
 * for their review. The author of the task that opened a request of the
 * user's keeps it (005).
 *
 * A row is the request and what it waits on: its checks, its review decision
 * and the threads nobody answered, then the session that works on it. The
 * repository, the author and the branches are the request's second line, so
 * the table reads at a glance without a column for each. The row holds no
 * button: a click opens the request's panel, which is where everything that
 * can be done to it is — an Ariadne review of a request of the user's among
 * it. Nothing is added by hand on this screen: the forge's lists are the list.
 */

import { useMutation, useQuery } from "@tanstack/react-query"
import { RefreshCwIcon } from "lucide-react"
import { Link, useLocation, useNavigate, useSearchParams } from "react-router-dom"

import { api, type PullRequestDto, unwrap } from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { StatusBadge } from "@/components/status-badge"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import {
  ForgeFilters,
  forgeName,
  matchesText,
  useNeedsRefresh,
  useSearchFilter,
} from "@/features/forge/forge-filters"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { paths, sessionPanelFrom } from "@/routes/paths"

import { pullRequestsQueryOptions } from "./queries"
import { ARIADNE_REVIEWING, CHECKS, REVIEW } from "./status"

/**
 * Whose requests the list holds: every open one of the repositories, the
 * user's own, or the ones that ask for their review.
 */
const WHOSE = [
  { value: null, label: "All", query: {} },
  { value: "author", label: "Mine", query: { role: "author" } },
  { value: "reviewer", label: "Review requests", query: { role: "reviewer", requested: true } },
] as const

const COLUMNS = [
  { header: "Request", className: "min-w-72" },
  { header: "Checks" },
  { header: "Review" },
  { header: "Unanswered", className: "text-right" },
  { header: "Session" },
  { header: "Updated" },
]

export function PullRequestsPage() {
  const [search] = useSearchParams()
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const [repo, setRepo] = useSearchFilter("repo")
  const [role, setRole] = useSearchFilter("role")
  const whose = WHOSE.find((each) => each.value === role) ?? WHOSE[0]
  const [text, setText] = useSearchFilter("q")
  const filters = { repo: repo ?? undefined, ...whose.query }
  const rows = useQuery(pullRequestsQueryOptions(filters))
  const repositories = useQuery(repositoriesQueryOptions())
  const enabled = repositories.data?.filter((each) => each.forge?.enabled) ?? []
  const needsRefresh = useNeedsRefresh(repo ? enabled.filter((each) => each.id === repo) : enabled)
  const refresh = useMutation({
    mutationFn: () =>
      unwrap(
        api().POST("/v1/pull-requests/refresh", { params: { query: { repo: filters.repo } } }),
      ),
  })

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Pull requests"
        description="The open pull requests of your repositories: yours, and the ones that ask for your review. A request a task opened is kept by that task's author."
        actions={
          // A live webhook brings every change here on its own: only a
          // repository that polls, or whose hook is down, waits for one.
          needsRefresh ? (
            <Button variant="outline" pending={refresh.isPending} onClick={() => refresh.mutate()}>
              <RefreshCwIcon />
              Refresh
            </Button>
          ) : null
        }
      />
      <ForgeFilters
        repositories={enabled}
        repository={repo}
        onRepository={setRepo}
        query={text}
        onQuery={setText}
        placeholder="Filter by title or description"
      >
        <ButtonGroup aria-label="Whose requests">
          {WHOSE.map((each) => (
            <Button
              key={each.label}
              variant={each === whose ? "secondary" : "outline"}
              aria-pressed={each === whose}
              onClick={() => setRole(each.value)}
            >
              {each.label}
            </Button>
          ))}
        </ButtonGroup>
      </ForgeFilters>
      {refresh.error ? <ErrorState title="Could not refresh" error={refresh.error} /> : null}
      <DataTable
        query={{
          ...rows,
          data: rows.data?.filter((row) =>
            matchesText(text, [
              `#${row.number}`,
              row.title,
              row.body,
              row.author_login,
              row.head_branch,
            ]),
          ),
        }}
        errorTitle="Could not load pull requests"
        columns={COLUMNS}
        empty={
          <EmptyState
            emphasis="quiet"
            className="border-0 py-10"
            title={
              whose.value === "author"
                ? "You have no open pull requests"
                : whose.value === "reviewer"
                  ? "No pull requests ask for your review"
                  : "No open pull requests"
            }
            description="The open requests of the repositories with their forge enabled show up here."
          />
        }
        rowKey={(row) => row.id}
        renderRow={(row) => (
          <PullRequestRow
            row={row}
            repository={forgeName(repositories.data?.find((r) => r.id === row.repository_id))}
            sessionTo={row.session_id ? sessionPanelFrom(pathname, search, row.session_id) : null}
            onInspect={() => navigate(paths.pullRequest(row.id, search))}
          />
        )}
      />
    </div>
  )
}

function PullRequestRow({
  row,
  repository,
  sessionTo,
  onInspect,
}: {
  row: PullRequestDto
  repository: string
  sessionTo: ReturnType<typeof sessionPanelFrom> | null
  onInspect: () => void
}) {
  const checks = CHECKS[row.checks]
  const review = REVIEW[row.review_decision]
  return (
    <TableRow
      onClick={onInspect}
      onKeyDown={(event) => {
        if (event.key === "Enter" && event.target === event.currentTarget) onInspect()
      }}
      tabIndex={0}
      aria-label={`Open #${row.number} ${row.title}`}
      className="cursor-pointer"
    >
      <TableCell className="max-w-md whitespace-normal">
        <div className="flex flex-col gap-0.5">
          <span className="flex flex-wrap items-center gap-2">
            <a
              href={row.url}
              target="_blank"
              rel="noreferrer"
              onClick={(event) => event.stopPropagation()}
              className="font-medium underline-offset-3 hover:underline"
            >
              #{row.number} {row.title}
            </a>
            {row.draft ? <StatusBadge size="sm" box="outlined" label="Draft" /> : null}
            {row.review_asked ? (
              <StatusBadge
                size="sm"
                label={ARIADNE_REVIEWING.label}
                tone={ARIADNE_REVIEWING.tone}
              />
            ) : null}
          </span>
          <span className="text-xs text-muted-foreground">
            <span className="font-mono">{repository || row.repository_id}</span> ·{" "}
            {row.role === "author" ? "by you" : `by ${row.author_login}`} ·{" "}
            <span className="font-mono">{row.head_branch}</span> →{" "}
            <span className="font-mono">{row.base_branch}</span>
            {row.tracked_by === "user" ? " · added by hand" : ""}
          </span>
        </div>
      </TableCell>
      <TableCell>
        {checks ? (
          <StatusBadge size="sm" label={checks.label} tone={checks.tone} dot={checks.dot} />
        ) : (
          <span className="text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell>
        {review ? (
          <StatusBadge size="sm" label={review.label} tone={review.tone} />
        ) : (
          <span className="text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell
        className={
          row.unanswered_comments > 0
            ? "text-right font-medium tabular-nums"
            : "text-right text-muted-foreground tabular-nums"
        }
      >
        {row.unanswered_comments}
      </TableCell>
      {/* The session that works on the request: its own panel, over this
          screen. */}
      <TableCell onClick={(event) => event.stopPropagation()}>
        {sessionTo ? (
          <Link
            to={sessionTo}
            aria-label={`Open the session of #${row.number}`}
            className="text-sm underline-offset-3 hover:underline"
          >
            Open
          </Link>
        ) : (
          <span className="text-muted-foreground">-</span>
        )}
      </TableCell>
      <TableCell className="text-muted-foreground">
        <When at={row.updated_at} format="age" label="updated" />
      </TableCell>
    </TableRow>
  )
}
