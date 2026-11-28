import { useQuery } from "@tanstack/react-query"
import { Skeleton } from "@/components/ui/skeleton"
import { tunnelQueryOptions } from "@/features/repositories/queries"
import { useHostSelection } from "@/hooks/use-host-filter"
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
 * it, and babysitting one by hand (032). The daemon sends the list a page of
 * 25 at a time, the text filter applied to every request first, and the next
 * page loads as the table scrolls near its end. Every `pull_requests_changed`
 * reads it again from its first page; nothing adds or removes a request.
 *
 * Under many hosts a request every host lists is one row, matched by its forge
 * repository and its number. The row reads the newest forge facts among them,
 * names the host of each Ariadne review and keeper, and links the session of
 * each host that has one.
 */

import { useMutation } from "@tanstack/react-query"
import { RefreshCwIcon } from "lucide-react"
import { useEffect, useRef } from "react"
import { Link, useLocation, useNavigate, useSearchParams } from "react-router-dom"

import { api, type Host, type HostRow, LOCAL_HOST, type PullRequestDto, unwrap } from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { HostLabel } from "@/components/host-label"
import { PageHeader } from "@/components/page-header"
import { PrLink } from "@/components/pr-link"
import { StatusBadge } from "@/components/status-badge"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import { TableCell, TableRow } from "@/components/ui/table"
import { When } from "@/components/when"
import {
  ForgeFilters,
  forgeName,
  useNeedsRefresh,
  useSearchFilter,
} from "@/features/forge/forge-filters"
import {
  distinctEnabled,
  filterOption,
  forgeItemKey,
  groupForgeItems,
  repositoryOn,
  useHostRepositories,
} from "@/features/forge/forge-hosts"
import { useHostFilter } from "@/hooks/use-host-filter"
import { useRowsShowHost } from "@/hooks/use-host-label"
import { paths, sessionPanelFrom } from "@/routes/paths"

import { pullRequestKey, usePullRequestLists } from "./queries"
import { ARIADNE_BABYSITTING, ARIADNE_REVIEWING, CHECKS, REVIEW } from "./status"

/**
 * Whose requests the list holds: every open one of the repositories, the
 * user's own, or the ones that ask for their review.
 */
const WHOSE = [
  { value: null, label: "All", query: {} },
  { value: "author", label: "Mine", query: { role: "author" } },
  { value: "reviewer", label: "Review requests", query: { role: "reviewer", requested: true } },
] as const

/** The rows of one request, one per host that lists it, the newest first. */
type Listing = HostRow<PullRequestDto>[]

/** Whether Ariadne works on the request on its row's host. */
function holdsWork(row: PullRequestDto): boolean {
  return Boolean(
    row.review_asked ||
      row.babysit_asked ||
      row.session_id ||
      row.origin_task_id ||
      (row.operations?.length ?? 0) > 0,
  )
}

/** The row whose host a click opens the panel on: one with Ariadne work, then `local`. */
function panelRow(listing: Listing): HostRow<PullRequestDto> {
  const working = listing.filter(holdsWork)
  const among = working.length > 0 ? working : listing
  return among.find((row) => row.host === LOCAL_HOST) ?? (among[0] as HostRow<PullRequestDto>)
}

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
  // The hosts the header picker selects. The repository filter is one forge
  // repository, which each host knows under an id of its own.
  const hosts = useHostFilter()
  const { isPending: hostsPending } = useHostSelection()
  const tunnel = useQuery(tunnelQueryOptions())
  const showHost = useRowsShowHost()
  const repositories = useHostRepositories(hosts)
  const enabled = distinctEnabled(repositories.rows)
  const queried = hosts.flatMap((host) => {
    const id = repo ? repositoryOn(repositories.known, repo, host) : undefined
    if (repo && id === undefined) return []
    return [{ host, filters: { repo: id, ...whose.query, q: text?.trim() || undefined } }]
  })
  const rows = usePullRequestLists(queried)
  const listed = rows.rows
    ? groupForgeItems(rows.rows, (row) =>
        forgeItemKey(repositories.known, row.host, row.repository_id, row.number),
      )
    : undefined
  const needsRefresh = useNeedsRefresh(
    repositories.rows.filter(
      (each) =>
        each.forge?.enabled &&
        (!repo || repositoryOn(repositories.known, repo, each.host) === each.id),
    ),
  )
  const refresh = useMutation({
    mutationFn: () =>
      Promise.all(
        queried.map(({ host, filters }) =>
          unwrap(
            api(host).POST("/v1/pull-requests/refresh", {
              params: { query: { repo: filters.repo } },
            }),
          ),
        ),
      ),
  })
  const query = {
    data: listed,
    isPending: rows.isPending,
    isError: rows.errors.length > 0,
    error: rows.errors[0]?.error,
    refetch: rows.refetch,
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Pull requests"
        description="The open pull requests of your repositories: yours, and the ones that ask for your review. A request a task opened is kept by that task's author."
        actions={
          // A live webhook brings every change here on its own: only a
          // repository that polls, or whose hook is down, waits for one.
          hostsPending || !repositories.isSettled || tunnel.isPending ? (
            <Skeleton aria-label="Loading refresh" className="h-9 w-24" />
          ) : needsRefresh ? (
            <Button variant="outline" pending={refresh.isPending} onClick={() => refresh.mutate()}>
              <RefreshCwIcon />
              Refresh
            </Button>
          ) : null
        }
      />
      <ForgeFilters
        repositories={enabled}
        repository={filterOption(enabled, repositories.known, repo)}
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
        query={query}
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
        rowKey={(listing) => {
          const row = listing[0] as HostRow<PullRequestDto>
          return forgeItemKey(repositories.known, row.host, row.repository_id, row.number)
        }}
        renderRow={(listing) => {
          const row = listing[0] as HostRow<PullRequestDto>
          const opens = panelRow(listing)
          return (
            <PullRequestRow
              row={row}
              repositoryPending={hostsPending || !repositories.isSettled}
              repository={forgeName(
                repositories.rows.find((r) => r.host === row.host && r.id === row.repository_id),
              )}
              reviews={listing.filter((each) => each.review_asked).map((each) => each.host)}
              keepers={listing.filter((each) => each.babysit_asked).map((each) => each.host)}
              showHost={showHost}
              sessions={listing.flatMap((each) =>
                each.session_id
                  ? [
                      {
                        host: each.host,
                        to: sessionPanelFrom(pathname, search, each.session_id, each.host),
                      },
                    ]
                  : [],
              )}
              onInspect={() =>
                navigate(paths.pullRequest(pullRequestKey(opens), search, opens.host))
              }
            />
          )
        }}
      />
      <NearEnd
        armed={rows.hasNextPage && !rows.isFetchingNextPage && rows.errors.length === 0}
        pages={rows.pages}
        onReach={() => rows.fetchNextPage()}
      />
    </div>
  )
}

/** How close to the end of the table the next page starts to load. */
const NEAR_END_MARGIN = "0px 0px 400px 0px"

/**
 * A mark under the table that asks for the next page once it scrolls within
 * {@link NEAR_END_MARGIN} of the view. The observer is made again for each
 * new page: a mark still in view after a short page reports it at once.
 */
function NearEnd({
  armed,
  pages,
  onReach,
}: {
  armed: boolean
  pages: number
  onReach: () => void
}) {
  const mark = useRef<HTMLDivElement>(null)
  const reach = useRef(onReach)
  reach.current = onReach
  // A new page is a new observer, which reports where the mark is now.
  // biome-ignore lint/correctness/useExhaustiveDependencies: `pages` re-arms the observer
  useEffect(() => {
    const node = mark.current
    if (!armed || !node || typeof IntersectionObserver === "undefined") return
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) reach.current()
      },
      { rootMargin: NEAR_END_MARGIN },
    )
    observer.observe(node)
    return () => observer.disconnect()
  }, [armed, pages])
  return <div ref={mark} aria-hidden="true" />
}

function PullRequestRow({
  row,
  repository,
  repositoryPending,
  reviews,
  keepers,
  showHost,
  sessions,
  onInspect,
}: {
  /** The newest of the request's rows: its forge facts. */
  row: HostRow<PullRequestDto>
  repositoryPending: boolean
  repository: string
  /** The hosts where Ariadne reviews the request, and where a keeper keeps it. */
  reviews: Host[]
  keepers: Host[]
  /** Whether the row names the host of each review, keeper and session. */
  showHost: boolean
  sessions: { host: Host; to: ReturnType<typeof sessionPanelFrom> }[]
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
            <PrLink
              url={row.url}
              number={row.number}
              context={row.title}
              onClick={(event) => event.stopPropagation()}
              className="font-medium"
            />
            <span className="font-medium">{row.title}</span>
            {row.draft ? <StatusBadge size="sm" box="outlined" label="Draft" /> : null}
            {(showHost ? reviews : reviews.slice(0, 1)).map((host) => (
              <span key={`review-${host}`} data-work className="inline-flex items-center gap-1">
                <StatusBadge
                  size="sm"
                  label={ARIADNE_REVIEWING.label}
                  tone={ARIADNE_REVIEWING.tone}
                />
                {showHost ? <HostLabel host={host} /> : null}
              </span>
            ))}
            {(showHost ? keepers : keepers.slice(0, 1)).map((host) => (
              <span key={`keeper-${host}`} data-work className="inline-flex items-center gap-1">
                <StatusBadge
                  size="sm"
                  label={ARIADNE_BABYSITTING.label}
                  tone={ARIADNE_BABYSITTING.tone}
                />
                {showHost ? <HostLabel host={host} /> : null}
              </span>
            ))}
          </span>
          <span className="text-xs text-muted-foreground">
            {repositoryPending ? (
              <Skeleton aria-label="Loading repository" className="inline-block h-4 w-28" />
            ) : (
              <span className="font-mono">{repository || row.repository_id}</span>
            )}{" "}
            · {row.role === "author" ? "by you" : `by ${row.author_login}`} ·{" "}
            <span className="font-mono">{row.head_branch}</span> →{" "}
            <span className="font-mono">{row.base_branch}</span>
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
          (row.unanswered_comments ?? 0) > 0
            ? "text-right font-medium tabular-nums"
            : "text-right text-muted-foreground tabular-nums"
        }
      >
        {row.unanswered_comments ?? 0}
      </TableCell>
      {/* The session that works on the request: its own panel, over this
          screen. */}
      <TableCell onClick={(event) => event.stopPropagation()}>
        {sessions.length > 0 ? (
          <span className="flex flex-wrap items-center gap-2">
            {sessions.map((session) => (
              <Link
                key={session.host}
                to={session.to}
                aria-label={
                  showHost
                    ? `Open the session of #${row.number} on ${session.host}`
                    : `Open the session of #${row.number}`
                }
                className="text-sm underline-offset-3 hover:underline"
              >
                {showHost ? session.host : "Open"}
              </Link>
            ))}
          </span>
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
