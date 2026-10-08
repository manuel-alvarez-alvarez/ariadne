/**
 * One pull request in full, in the side panel over the Forge screen: what
 * `pr inspect` prints, read as a person reads it — the request's state,
 * checks and review in its header, the facts that matter below with links
 * where they point somewhere, its description rendered, and the sessions
 * that work on it.
 *
 * Everything that can be done to a request is here rather than on its row:
 * ask Ariadne to review a request of the user's own and stop asking (029).
 * Nothing adds or removes a request: Ariadne works on one while it reviews
 * it or a task keeps it, and lets go of it once it merged or closed.
 *
 * The request is read off the forge each time the panel opens, by its
 * repository and number, and read again on every `pull_requests_changed`.
 *
 * The sessions are the task panel's: a review session Ariadne runs on the
 * request, and the author of the task that keeps it (005). Picking one drills
 * into it, as `?session=` beside `?pr=`, and the panel becomes that session's
 * with a way back to the request.
 */

import { useQuery } from "@tanstack/react-query"
import { BotIcon, BotOffIcon, ExternalLinkIcon } from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { PullRequestDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Fact, FactList } from "@/components/fact-list"
import { Markdown } from "@/components/markdown"
import { PanelHeader } from "@/components/panel-header"
import { PanelSheet } from "@/components/panel-sheet"
import { StatusBadge } from "@/components/status-badge"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { PaneBody, PaneHeader, PaneTitle } from "@/components/ui/docked-pane"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
import { forgeName } from "@/features/forge/forge-filters"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { sessionQueryOptions, sessionsQueryOptions } from "@/features/sessions/queries"
import { SessionDetailView, SessionPanelHeader } from "@/features/sessions/session-detail-view"
import { isLiveStatus } from "@/features/sessions/session-display"
import { SessionsList } from "@/features/sessions/sessions-list"
import { useFocusReturn } from "@/hooks/use-focus-return"
import { shortId, shortSha } from "@/lib/format"
import { usePanelSessionNavigation, useTaskPanelTo } from "@/routes/paths"

import { pullRequestByKeyQueryOptions, useAskReview } from "./queries"
import { StartReviewDialog } from "./start-review-dialog"
import { ARIADNE_REVIEWING, CHECKS, REVIEW } from "./status"

const TABS = ["description", "sessions"] as const
type Tab = (typeof TABS)[number]

export function PullRequestPanel({ id, onClose }: { id: string; onClose: () => void }) {
  const [search, setSearch] = useSearchParams()
  const query = useQuery(pullRequestByKeyQueryOptions(id))
  // Ariadne's id of the request, while it works on it: what its sessions
  // name.
  const workedId = query.data?.id ?? null
  const session = search.get("session") ?? undefined
  const selectSession = usePanelSessionNavigation()
  const tab = TABS.find((value) => value === search.get("tab")) ?? "description"
  const panel = useRef<HTMLDivElement>(null)
  useFocusReturn(session ?? null, panel)
  const opened = useReviewOpening(workedId, (sessionId) => {
    const params = new URLSearchParams(search)
    params.set("tab", "sessions")
    params.set("session", sessionId)
    setSearch(params, { replace: true })
  })

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <PanelSheet onClose={onClose} panelRef={panel}>
      {session ? (
        <PullRequestSessionView
          pull={query.data}
          pullRequestId={workedId}
          sessionId={session}
          onSelect={selectSession}
        />
      ) : query.isPending ? (
        <>
          <PaneHeader>
            <PaneTitle className="sr-only">Loading pull request</PaneTitle>
          </PaneHeader>
          <PaneBody>
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-28 w-full" />
            <Skeleton className="h-64 w-full" />
          </PaneBody>
        </>
      ) : query.isError ? (
        <>
          <PaneHeader>
            <PaneTitle className="sr-only">Pull request {id}</PaneTitle>
          </PaneHeader>
          <PaneBody>
            <ErrorState
              title="Could not load pull request"
              error={query.error}
              onRetry={() => void query.refetch()}
            />
          </PaneBody>
        </>
      ) : (
        <>
          <PullRequestHeader
            pull={query.data}
            onReviewStarted={() => {
              opened.await()
              setTab("sessions")
            }}
          />
          <PaneBody>
            <PullRequestFacts pull={query.data} />
            <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
              <TabsList>
                <TabsTrigger value="description">Description</TabsTrigger>
                <TabsTrigger value="sessions">Sessions</TabsTrigger>
              </TabsList>
              <TabsContent value="description" className="pt-3">
                {query.data.body?.trim() ? (
                  <Markdown>{query.data.body}</Markdown>
                ) : (
                  <EmptyState emphasis="quiet" title="No description" />
                )}
              </TabsContent>
              <TabsContent value="sessions" className="flex flex-col gap-4 pt-3">
                {opened.waiting ? (
                  <p className="text-sm text-muted-foreground" role="status">
                    Starting the review… its console opens here once the agent is up.
                  </p>
                ) : null}
                <PullRequestSessions pull={query.data} onSelect={selectSession} />
              </TabsContent>
            </Tabs>
          </PaneBody>
        </>
      )}
    </PanelSheet>
  )
}

/**
 * The review session a Start review asked for, opened once it exists: the
 * daemon starts it a moment after the ask (029), so the review sessions live
 * then are noted, and the first one live besides them is handed to `open` —
 * a new session, or the last one resumed. The list follows `session_created` on its own; the interval is
 * what holds while the stream is down.
 */
function useReviewOpening(pullRequestId: string | null, open: (sessionId: string) => void) {
  const [before, setBefore] = useState<string[] | null>(null)
  // Taken the moment a session is handed over, ahead of any render: a later
  // pass of the effect, on a list read again meanwhile, must not hand the
  // same session over a second time and pull the panel back into it.
  const pending = useRef(false)
  const reviews = useQuery({
    ...sessionsQueryOptions({ pull_request: pullRequestId ?? "" }),
    enabled: pullRequestId !== null,
    refetchInterval: before ? 2000 : false,
  })
  // A review comes up as a new session, or as the last one resumed on the
  // same pin: either way it is a session live now that was not at the ask.
  const live = (reviews.data ?? [])
    .filter((session) => isLiveStatus(session.status))
    .map((session) => session.id)
  const ids = live.join(" ")
  useEffect(() => {
    if (!before || !pending.current) return
    const fresh = ids.split(" ").find((each) => each && !before.includes(each))
    if (!fresh) return
    pending.current = false
    setBefore(null)
    open(fresh)
  }, [before, ids, open])
  return {
    waiting: before !== null,
    await: () => {
      pending.current = true
      setBefore(live)
    },
  }
}

/**
 * The title, the actions on the request, and its state in pills: open,
 * merged or closed, the checks, the review decision, and an Ariadne review
 * while one is asked.
 */
function PullRequestHeader({
  pull,
  onReviewStarted,
}: {
  pull: PullRequestDto
  /** A review was asked for in the dialog, which closed itself. */
  onReviewStarted: () => void
}) {
  const ask = useAskReview()
  const [starting, setStarting] = useState(false)
  const checks = CHECKS[pull.checks]
  const review = REVIEW[pull.review_decision]
  const mine = pull.role === "author"
  return (
    <>
      <PanelHeader
        title={`#${pull.number} ${pull.title}`}
        actions={
          mine && pull.state === "open" ? (
            pull.review_asked ? (
              <Button
                variant="outline"
                size="sm"
                pending={ask.isPending}
                onClick={() =>
                  ask.mutate({
                    repository_id: pull.repository_id,
                    number: pull.number,
                    asked: false,
                  })
                }
              >
                <BotOffIcon />
                Stop review
              </Button>
            ) : (
              <Button size="sm" onClick={() => setStarting(true)}>
                <BotIcon />
                Start review
              </Button>
            )
          ) : null
        }
        status={
          <>
            <StatusBadge
              label={
                pull.state === "open" ? (pull.draft ? "Draft" : "Open") : capitalize(pull.state)
              }
              box="outlined"
            />
            {checks ? (
              <StatusBadge label={checks.label} tone={checks.tone} dot={checks.dot} />
            ) : null}
            {review ? <StatusBadge label={review.label} tone={review.tone} /> : null}
            {pull.review_asked ? (
              <StatusBadge label={ARIADNE_REVIEWING.label} tone={ARIADNE_REVIEWING.tone} />
            ) : null}
          </>
        }
        id={pull.id ? <CopyableId value={pull.id} label="pull request id" /> : undefined}
        stamps={
          <>
            <span>opened</span>
            <When at={pull.opened_at} label="opened" />
            <span aria-hidden="true">·</span>
            <span>updated</span>
            <When at={pull.updated_at} label="updated" />
          </>
        }
      />
      <StartReviewDialog
        pull={pull}
        open={starting}
        onOpenChange={setStarting}
        onStarted={onReviewStarted}
      />
      {ask.error ? (
        <div className="px-4">
          <ErrorState title="Could not change Ariadne's review" error={ask.error} />
        </div>
      ) : null}
    </>
  )
}

function PullRequestFacts({ pull }: { pull: PullRequestDto }) {
  const repositories = useQuery(repositoriesQueryOptions())
  const repository = repositories.data?.find((each) => each.id === pull.repository_id)
  const failed = pull.failed_checks ?? []
  return (
    <FactList>
      <Fact label="Request">
        <a
          href={pull.url}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-1 underline-offset-3 hover:underline"
        >
          #{pull.number} on the forge
          <ExternalLinkIcon className="size-3.5" />
        </a>
      </Fact>
      <Fact label="Repository">
        <span className="font-mono text-sm">{forgeName(repository) || pull.repository_id}</span>
      </Fact>
      <Fact label="Author">{pull.role === "author" ? "You" : pull.author_login}</Fact>
      <Fact label="Branches" className="sm:col-span-2">
        <span className="font-mono text-sm break-all">
          {pull.head_branch} → {pull.base_branch}
        </span>
      </Fact>
      <Fact label="Head" hint={pull.head_sha}>
        <span className="font-mono text-sm">{shortSha(pull.head_sha)}</span>
      </Fact>
      <Fact label="Unanswered comments">{pull.unanswered_comments}</Fact>
      <Fact label="Base">{pull.behind_base ? "Ahead of the head" : "Level with the head"}</Fact>
      <Fact label="Ariadne">{ariadneWork(pull)}</Fact>
      {pull.origin_task_id ? (
        <Fact label="Kept by">
          <TaskLink taskId={pull.origin_task_id} />
        </Fact>
      ) : null}
      {failed.length > 0 ? (
        <Fact label="Failed checks" className="sm:col-span-2 lg:col-span-3">
          <ul className="flex flex-col gap-1">
            {failed.map((check) => (
              <li key={`${check.name}:${check.url}`}>
                {check.url ? (
                  <a
                    href={check.url}
                    target="_blank"
                    rel="noreferrer"
                    className="underline-offset-3 hover:underline"
                  >
                    {check.name}
                  </a>
                ) : (
                  check.name
                )}{" "}
                <span className="text-muted-foreground">({check.conclusion})</span>
              </li>
            ))}
          </ul>
        </Fact>
      ) : null}
    </FactList>
  )
}

/** What Ariadne does with the request. */
function ariadneWork(pull: PullRequestDto): string {
  if (pull.origin_task_id) return "Keeps it for a task"
  if (pull.id) return "Reviews it"
  return "Works on nothing here"
}

/** The task that opened the request, whose author keeps it, as its panel. */
function TaskLink({ taskId }: { taskId: string }) {
  const to = useTaskPanelTo(taskId)
  return (
    <Link to={to} replace={to.replace} className="underline-offset-3 hover:underline">
      the author of task {shortId(taskId)}
    </Link>
  )
}

/**
 * The sessions that work on the request: the review sessions Ariadne runs
 * on it, and the author of the task that keeps it.
 */
function PullRequestSessions({
  pull,
  onSelect,
}: {
  pull: PullRequestDto
  onSelect: (sessionId: string) => void
}) {
  return (
    <>
      <section className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Ariadne review</h3>
        {pull.id ? (
          <SessionsList
            filters={{ pull_request: pull.id }}
            onSelect={(session) => onSelect(session.id)}
          />
        ) : (
          <EmptyState emphasis="quiet" title="No Ariadne review of this request yet" />
        )}
      </section>
      {pull.origin_task_id ? (
        <section className="flex flex-col gap-2">
          <h3 className="text-sm font-medium">The task's author</h3>
          <SessionsList
            filters={{ task: pull.origin_task_id, seat: "author" }}
            onSelect={(session) => onSelect(session.id)}
          />
        </section>
      ) : null}
    </>
  )
}

/**
 * The selected session as the panel's whole body, fetched by id: a session
 * of this request, its review or its task's author, with a way back to the
 * request. One of anything else is not shown as if it were.
 */
function PullRequestSessionView({
  pull,
  pullRequestId,
  sessionId,
  onSelect,
}: {
  pull: PullRequestDto | undefined
  pullRequestId: string | null
  sessionId: string
  onSelect: (sessionId: string | null) => void
}) {
  const session = useQuery(sessionQueryOptions(sessionId))
  const ours =
    session.data !== undefined &&
    ((pullRequestId !== null && session.data.pull_request_id === pullRequestId) ||
      (pull?.origin_task_id != null && session.data.task_id === pull.origin_task_id))
  const breadcrumb = {
    label: pull ? `#${pull.number} ${pull.title}` : "pull request",
    onClick: () => onSelect(null),
  }
  return (
    <>
      {session.data && ours ? (
        <SessionPanelHeader
          session={session.data}
          breadcrumb={breadcrumb}
          onResumed={(revived) => onSelect(revived.id)}
          onSwitched={(successor) => onSelect(successor.id)}
        />
      ) : (
        <PanelHeader breadcrumb={breadcrumb} title={`Session ${shortId(sessionId)}`} />
      )}
      <PaneBody>
        {session.isPending ? (
          <div className="space-y-3">
            <Skeleton className="h-8 w-64" />
            <Skeleton className="h-32 w-full" />
            <Skeleton className="h-72 w-full" />
          </div>
        ) : session.error ? (
          <ErrorState
            title={`Could not load session ${shortId(sessionId)}`}
            error={session.error}
            onRetry={() => void session.refetch()}
          />
        ) : !ours ? (
          <Alert variant="destructive">
            <AlertTitle>Not a session of this request</AlertTitle>
            <AlertDescription>
              Session {shortId(sessionId)} works on something else, so it is not shown here.
            </AlertDescription>
          </Alert>
        ) : (
          <SessionDetailView session={session.data} context="task" />
        )}
      </PaneBody>
    </>
  )
}

function capitalize(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1)
}
