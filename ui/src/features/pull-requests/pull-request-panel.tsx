/**
 * One pull request in full, in the side panel over the Forge screen: what
 * `pr inspect` prints, read as a person reads it — the request's state,
 * checks and review in its header, the facts that matter below with links
 * where they point somewhere, its description rendered, and the sessions
 * that work on it.
 *
 * Everything that can be done to a request is here rather than on its row:
 * ask Ariadne to review any open request and stop asking (029),
 * or to babysit one by hand and stop that too (032). Nothing adds or
 * removes a request: Ariadne works on one while it reviews it, babysits it
 * or a task keeps it, and lets go of it once it merged or closed.
 *
 * The request is read off the forge each time the panel opens, by its
 * repository and number, and read again on every `pull_requests_changed`.
 *
 * Every reachable host that has the request's forge repository reads it by
 * its own repository id, and the panel shows the work of all of them: who
 * reviews it and who keeps it, and their sessions. Each action goes to the
 * host that holds the work.
 *
 * The sessions are the task panel's: a review or a keeper session Ariadne
 * runs on the request, and the author of the task that keeps it (005).
 * Picking one drills into it, as `?session=` beside `?pr=`, and the panel
 * becomes that session's with a way back to the request. A session of
 * another host moves `?host=` and `?pr=` to that host's copy of the request.
 */

import { useQuery, useQueryClient } from "@tanstack/react-query"
import { BotIcon, BotOffIcon } from "lucide-react"
import { type ReactNode, useEffect, useRef, useState } from "react"
import { Link, useSearchParams } from "react-router-dom"

import {
  type Host,
  type HostRow,
  LOCAL_HOST,
  type PullRequestDto,
  type Seat,
  type SessionDto,
} from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState, HostListErrors } from "@/components/error-state"
import { Fact, FactList } from "@/components/fact-list"
import { HostLabel } from "@/components/host-label"
import { Markdown } from "@/components/markdown"
import { PanelHeader } from "@/components/panel-header"
import { PanelSheet } from "@/components/panel-sheet"
import { PrLink } from "@/components/pr-link"
import { StatusBadge } from "@/components/status-badge"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import { PaneBody, PaneHeader, PaneTitle } from "@/components/ui/docked-pane"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
import { forgeName } from "@/features/forge/forge-filters"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { sessionQueryOptions, sessionsQueryOptions } from "@/features/sessions/queries"
import { SessionDetailView, SessionPanelHeader } from "@/features/sessions/session-detail-view"
import { isLiveStatus } from "@/features/sessions/session-display"
import { HostSessionsList } from "@/features/sessions/sessions-list"
import { useFocusReturn } from "@/hooks/use-focus-return"
import { usePanelShowsHost } from "@/hooks/use-host-label"
import { shortId, shortSha } from "@/lib/format"
import { hostOf, paths, usePanelSessionNavigation, useTaskPanelTo } from "@/routes/paths"

import {
  pullRequestByKeyQueryOptions,
  pullRequestKey,
  pullRequestQueryOptions,
  useAskBabysit,
  useAskReview,
  useKeeperTurn,
  useReviewRound,
} from "./queries"
import { useRequestCopies } from "./review-hosts"
import { StartBabysitDialog } from "./start-babysit-dialog"
import { StartReviewDialog } from "./start-review-dialog"
import { ARIADNE_BABYSITTING, ARIADNE_REVIEWING, CHECKS, REVIEW } from "./status"

const TABS = ["description", "sessions"] as const
type Tab = (typeof TABS)[number]

export function PullRequestPanel({ id, onClose }: { id: string; onClose: () => void }) {
  const [search, setSearch] = useSearchParams()
  // The host the request is on, from `?host=` beside `?pr=`.
  const host = hostOf(search)
  const query = useQuery(
    id.includes(":") ? pullRequestByKeyQueryOptions(host, id) : pullRequestQueryOptions(host, id),
  )
  // Ariadne's id of the request, while it works on it: what its sessions
  // name.
  const workedId = query.data?.id ?? null
  const session = search.get("session") ?? undefined
  const selectSession = usePanelSessionNavigation()
  const tab = TABS.find((value) => value === search.get("tab")) ?? "description"
  const panel = useRef<HTMLDivElement>(null)
  useFocusReturn(session ?? null, panel)

  // A session of another host opens on that host's copy of the request, so
  // its console, input and actions go to the host that runs it.
  function openSession(on: Host, key: string, sessionId: string) {
    const to = paths.pullRequest(key, search, on)
    const params =
      on === host ? new URLSearchParams(search) : new URLSearchParams(to.slice(to.indexOf("?")))
    params.set("tab", "sessions")
    params.set("session", sessionId)
    setSearch(params, { replace: true })
  }
  const opened = useSessionOpening(openSession)

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <PanelSheet onClose={onClose} panelRef={panel}>
      {session ? (
        <PullRequestSessionView
          host={host}
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
        <PullRequestBody
          pull={query.data}
          host={host}
          tab={tab}
          onTab={setTab}
          waiting={opened.waiting}
          onSessionStarted={(seat, startedOn, started) => {
            opened.await(seat, startedOn, started)
            setTab("sessions")
          }}
          onSelect={(on, sessionId) => openSession(on.host, pullRequestKey(on), sessionId)}
        />
      )}
    </PanelSheet>
  )
}

/**
 * The request with the work of every host on it: each host that has its
 * repository reads it by that host's own id (`useRequestCopies`), and the
 * header, the facts and the sessions are of all of them.
 */
function PullRequestBody({
  pull,
  host,
  tab,
  onTab,
  waiting,
  onSessionStarted,
  onSelect,
}: {
  pull: PullRequestDto
  host: Host
  tab: Tab
  onTab: (tab: Tab) => void
  waiting: boolean
  onSessionStarted: (seat: Seat, startedOn: Host, started: PullRequestDto) => void
  onSelect: (on: HostRow<PullRequestDto>, sessionId: string) => void
}) {
  const { copies, settled, errors, refetch } = useRequestCopies(pull, host)
  return (
    <>
      <PullRequestHeader
        pull={pull}
        host={host}
        copies={copies}
        settled={settled}
        onSessionStarted={onSessionStarted}
      />
      <PaneBody>
        {errors.length > 0 ? (
          <HostListErrors
            what="the pull request"
            errors={errors}
            hostCount={copies.length + errors.length}
            hasRows
            onRetry={refetch}
          />
        ) : null}
        <PullRequestFacts pull={pull} host={host} copies={copies} />
        <Tabs value={tab} onValueChange={(value) => onTab(value as Tab)}>
          <TabsList>
            <TabsTrigger value="description">Description</TabsTrigger>
            <TabsTrigger value="sessions">Sessions</TabsTrigger>
          </TabsList>
          <TabsContent value="description" className="pt-3">
            {pull.body?.trim() ? (
              <Markdown>{pull.body}</Markdown>
            ) : (
              <EmptyState emphasis="quiet" title="No description" />
            )}
          </TabsContent>
          <TabsContent value="sessions" className="flex flex-col gap-4 pt-3">
            {waiting ? (
              <p className="text-sm text-muted-foreground" role="status">
                Starting the session… its console opens here once the agent is up.
              </p>
            ) : null}
            <PullRequestSessions copies={copies} onSelect={onSelect} />
          </TabsContent>
        </Tabs>
      </PaneBody>
    </>
  )
}

/**
 * The session a Start review or Start babysit asked for, opened once it
 * exists: the daemon starts it a moment after the ask (029, 032), so the
 * live sessions of the request on that seat, then, are noted, and the
 * first one live besides them is handed to `open` — a new session, or the
 * last one resumed. The seat kept apart is what stops a review that resumes
 * mid-wait from being handed over in place of a keeper, and the reverse.
 * The sessions are read on the host the work started on, by the id that
 * host gave the request. The list follows `session_created` on its own; the
 * interval is what holds while the stream is down.
 */
function useSessionOpening(open: (host: Host, key: string, sessionId: string) => void) {
  const client = useQueryClient()
  const [asked, setAsked] = useState<{
    host: Host
    pull: PullRequestDto
    seat: Seat
    before: string[]
  } | null>(null)
  // Taken the moment a session is handed over, ahead of any render: a later
  // pass of the effect, on a list read again meanwhile, must not hand the
  // same session over a second time and pull the panel back into it.
  const pending = useRef(false)
  const reviews = useQuery({
    ...sessionsQueryOptions(asked?.host ?? LOCAL_HOST, { pull_request: asked?.pull.id ?? "" }),
    enabled: asked?.pull.id != null,
    refetchInterval: asked ? 2000 : false,
  })
  // A review or a keeper comes up as a new session, or as the last one
  // resumed on the same pin: either way it is a session live now, on the
  // seat asked for, that was not at the ask.
  const ids = asked ? liveOn(reviews.data, asked.seat).join(" ") : ""
  useEffect(() => {
    if (!asked || !pending.current) return
    const fresh = ids.split(" ").find((each) => each && !asked.before.includes(each))
    if (!fresh) return
    pending.current = false
    setAsked(null)
    open(asked.host, pullRequestKey(asked.pull), fresh)
  }, [asked, ids, open])
  return {
    waiting: asked !== null,
    await: (seat: Seat, host: Host, pull: PullRequestDto) => {
      pending.current = true
      const known = pull.id
        ? client.getQueryData(sessionsQueryOptions(host, { pull_request: pull.id }).queryKey)
        : undefined
      setAsked({ host, pull, seat, before: liveOn(known, seat) })
    },
  }
}

/** The ids of the live sessions on `seat`. */
function liveOn(sessions: SessionDto[] | undefined, seat: Seat): string[] {
  return (sessions ?? [])
    .filter((session) => isLiveStatus(session.status) && session.seat === seat)
    .map((session) => session.id)
}

/**
 * The title, the actions on the request, and its state in pills: open,
 * merged or closed, the checks, the review decision, an Ariadne review
 * while one is asked, and Ariadne babysitting while a keeper session is.
 *
 * Every review and every babysit start manually (029, 032), regardless of
 * authorship or review assignment. The work may be on any host that has the
 * repository, so each action goes to the host that holds it: a request one
 * host reviews offers no second review, and one a host or a task keeps
 * offers no second keeper.
 */
function PullRequestHeader({
  pull,
  host,
  copies,
  settled,
  onSessionStarted,
}: {
  pull: PullRequestDto
  /** The host the request is on. */
  host: Host
  /** The request as each host reads it, `host`'s first. */
  copies: HostRow<PullRequestDto>[]
  /** Every host that has the repository answered: no host left to hold the work unseen. */
  settled: boolean
  /** A review or a babysit was asked for in its dialog, which closed itself. */
  onSessionStarted: (seat: Seat, startedOn: Host, started: PullRequestDto) => void
}) {
  const showHost = usePanelShowsHost()
  const ask = useAskReview()
  const round = useReviewRound()
  const keeperTurn = useKeeperTurn()
  const babysit = useAskBabysit()
  const [starting, setStarting] = useState(false)
  const [babysitting, setBabysitting] = useState(false)
  const checks = CHECKS[pull.checks]
  const review = REVIEW[pull.review_decision]
  const reviewer = copies.find((copy) => copy.review_asked)
  const taskKeeper = copies.find((copy) => copy.origin_task_id != null)
  const babysitter = copies.find((copy) => copy.babysit_asked)
  const keeper = taskKeeper ?? babysitter
  // A start waits for every host: one not read yet may already hold the work.
  const canStartReview = settled && !reviewer
  const canStartBabysit = settled && !keeper
  // A host that reports the work closes a dialog opened before it answered.
  // A host that is read again, or fails and retries, only disables Start
  // until it answers: the dialog stays open.
  const reviewed = reviewer !== undefined
  const kept = keeper !== undefined
  useEffect(() => {
    if (reviewed) setStarting(false)
  }, [reviewed])
  useEffect(() => {
    if (kept) setBabysitting(false)
  }, [kept])
  // The review and the babysit are goals of their own, on the Goals board
  // like any other (029, 032), each on the host that runs it.
  const reviewGoal = operationOf(copies, "review")
  const babysitGoal = operationOf(copies, "babysit")
  const target = (copy: HostRow<PullRequestDto>) => ({
    host: copy.host,
    repository_id: copy.repository_id,
    number: copy.number,
  })
  return (
    <>
      <PanelHeader
        title={`#${pull.number} ${pull.title}`}
        actions={
          <div className="flex items-center gap-2">
            {pull.state === "open" ? (
              <>
                <ButtonGroup aria-label="Review">
                  {reviewer ? (
                    <>
                      <Button
                        variant="outline"
                        size="sm"
                        pending={ask.isPending}
                        onClick={() => ask.mutate({ ...target(reviewer), asked: false })}
                      >
                        <BotOffIcon />
                        Stop review
                      </Button>
                      <Button
                        variant="outline"
                        size="sm"
                        pending={round.isPending}
                        onClick={() => round.mutate(target(reviewer))}
                      >
                        New turn
                      </Button>
                    </>
                  ) : (
                    <Button size="sm" disabled={!settled} onClick={() => setStarting(true)}>
                      <BotIcon />
                      Start review
                    </Button>
                  )}
                </ButtonGroup>
                <ButtonGroup aria-label="Babysit">
                  {!taskKeeper && babysitter ? (
                    <Button
                      variant="outline"
                      size="sm"
                      pending={babysit.isPending}
                      onClick={() => babysit.mutate({ ...target(babysitter), asked: false })}
                    >
                      <BotOffIcon />
                      Stop babysit
                    </Button>
                  ) : null}
                  {keeper ? (
                    <Button
                      variant="outline"
                      size="sm"
                      pending={keeperTurn.isPending}
                      onClick={() => keeperTurn.mutate(target(keeper))}
                    >
                      New turn
                    </Button>
                  ) : (
                    <Button size="sm" disabled={!settled} onClick={() => setBabysitting(true)}>
                      <BotIcon />
                      Start babysit
                    </Button>
                  )}
                </ButtonGroup>
              </>
            ) : null}
          </div>
        }
        status={
          <>
            {showHost ? <HostLabel host={host} /> : null}
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
            {reviewer ? (
              <span className="inline-flex items-center gap-1" data-slot="ariadne-work">
                <StatusBadge label={ARIADNE_REVIEWING.label} tone={ARIADNE_REVIEWING.tone} />
                {showHost ? <HostLabel host={reviewer.host} /> : null}
              </span>
            ) : null}
            {reviewGoal ? (
              <Link
                className="text-xs underline"
                to={paths.goal(reviewGoal.goal_id, reviewGoal.host)}
              >
                Review goal
              </Link>
            ) : null}
            {babysitter ? (
              <span className="inline-flex items-center gap-1" data-slot="ariadne-work">
                <StatusBadge label={ARIADNE_BABYSITTING.label} tone={ARIADNE_BABYSITTING.tone} />
                {showHost ? <HostLabel host={babysitter.host} /> : null}
              </span>
            ) : null}
            {babysitGoal ? (
              <Link
                className="text-xs underline"
                to={paths.goal(babysitGoal.goal_id, babysitGoal.host)}
              >
                Babysit goal
              </Link>
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
        host={host}
        open={starting && !reviewed}
        canStart={canStartReview}
        onOpenChange={setStarting}
        onStarted={(startedOn, started) => onSessionStarted("reviewer", startedOn, started)}
      />
      <StartBabysitDialog
        pull={pull}
        host={host}
        open={babysitting && !kept}
        canStart={canStartBabysit}
        onOpenChange={setBabysitting}
        onStarted={(startedOn, started) => onSessionStarted("keeper", startedOn, started)}
      />
      {keeperTurn.error ? (
        <ErrorState title="Could not start a keeper turn" error={keeperTurn.error} />
      ) : null}
      {round.error ? (
        <div className="px-4">
          <ErrorState title="Could not start another review" error={round.error} />
        </div>
      ) : null}
      {ask.error ? (
        <div className="px-4">
          <ErrorState title="Could not change Ariadne's review" error={ask.error} />
        </div>
      ) : null}
      {babysit.error ? (
        <div className="px-4">
          <ErrorState title="Could not change Ariadne's babysitting" error={babysit.error} />
        </div>
      ) : null}
    </>
  )
}

/** The goal of the first host that runs `operation` on the request, with that host. */
function operationOf(
  copies: HostRow<PullRequestDto>[],
  operation: "review" | "babysit",
): { goal_id: string; host: Host } | undefined {
  for (const copy of copies) {
    const found = copy.operations?.find((each) => each.operation === operation)
    if (found) return { goal_id: found.goal_id, host: copy.host }
  }
  return undefined
}

function PullRequestFacts({
  pull,
  host,
  copies,
}: {
  pull: PullRequestDto
  host: Host
  copies: HostRow<PullRequestDto>[]
}) {
  const showHost = usePanelShowsHost()
  const repositories = useQuery(repositoriesQueryOptions(host))
  const repository = repositories.data?.find((each) => each.id === pull.repository_id)
  const failed = pull.failed_checks ?? []
  const taskKeeper = copies.find((copy) => copy.origin_task_id != null)
  return (
    <FactList>
      <Fact label="Request">
        <PrLink
          url={pull.url}
          number={pull.number}
          repository={forgeName(repository) || undefined}
          context={pull.title}
        />
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
      <Fact label="Ariadne">{ariadneWork(copies, showHost)}</Fact>
      {taskKeeper?.origin_task_id ? (
        <Fact label="Kept by">
          <TaskLink taskId={taskKeeper.origin_task_id} host={taskKeeper.host} />
          {showHost ? <HostLabel host={taskKeeper.host} className="ml-1.5" /> : null}
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

/**
 * What Ariadne does with the request, on each host that works on it, each
 * host named by rule 74.
 */
function ariadneWork(copies: HostRow<PullRequestDto>[], showHost: boolean): ReactNode {
  const work = copies.flatMap((copy) => {
    const what = copy.origin_task_id ? "Keeps it for a task" : copy.id ? "Reviews it" : null
    return what ? [{ what, host: copy.host }] : []
  })
  if (work.length === 0) return "Works on nothing here"
  return (
    <span className="flex flex-col gap-1">
      {work.map(({ what, host }) => (
        <span key={host} className="flex items-center gap-1.5" data-slot="ariadne-work">
          <span>{what}</span>
          {showHost ? <HostLabel host={host} /> : null}
        </span>
      ))}
    </span>
  )
}

/** The task that opened the request, whose author keeps it, as its panel. */
function TaskLink({ taskId, host }: { taskId: string; host: Host }) {
  const to = useTaskPanelTo(taskId, host)
  return (
    <Link to={to} replace={to.replace} className="underline-offset-3 hover:underline">
      the author of task {shortId(taskId)}
    </Link>
  )
}

/**
 * The sessions that work on the request, on every host that has it: the
 * review and the keeper sessions Ariadne runs on it (029, 032), and the
 * author of the task that keeps it.
 */
function PullRequestSessions({
  copies,
  onSelect,
}: {
  copies: HostRow<PullRequestDto>[]
  onSelect: (on: HostRow<PullRequestDto>, sessionId: string) => void
}) {
  const showHost = usePanelShowsHost()
  const worked = copies.filter((copy) => copy.id)
  const kept = copies.filter((copy) => copy.origin_task_id)
  const select = (session: SessionDto, at: Host) => {
    const on = copies.find((copy) => copy.host === at)
    if (on) onSelect(on, session.id)
  }
  return (
    <>
      <section className="flex flex-col gap-2">
        <h3 className="text-sm font-medium">Ariadne sessions</h3>
        {worked.length > 0 ? (
          <HostSessionsList
            lists={worked.map((copy) => ({
              host: copy.host,
              filters: { pull_request: copy.id as string },
            }))}
            showHost={showHost}
            onSelect={select}
          />
        ) : (
          <EmptyState emphasis="quiet" title="No Ariadne session on this request yet" />
        )}
      </section>
      {kept.length > 0 ? (
        <section className="flex flex-col gap-2">
          <h3 className="text-sm font-medium">Task workflow</h3>
          <HostSessionsList
            lists={kept.map((copy) => ({
              host: copy.host,
              filters: { task: copy.origin_task_id as string, seat: "agent" as const },
            }))}
            showHost={showHost}
            onSelect={select}
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
  host,
  pull,
  pullRequestId,
  sessionId,
  onSelect,
}: {
  host: Host
  pull: PullRequestDto | undefined
  pullRequestId: string | null
  sessionId: string
  onSelect: (sessionId: string | null) => void
}) {
  const session = useQuery(sessionQueryOptions(host, sessionId))
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
          host={host}
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
          <SessionDetailView host={host} session={session.data} context="task" />
        )}
      </PaneBody>
    </>
  )
}

function capitalize(word: string): string {
  return word.charAt(0).toUpperCase() + word.slice(1)
}
