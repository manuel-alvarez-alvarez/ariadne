/**
 * One goal in a side panel over the goals board: what it is, what it is
 * allowed to do, its planner thread and the sessions it has run.
 * `ariadne goal inspect` plus `goal messages` — its tasks stay on the board
 * behind the panel.
 *
 * Everything reads the query cache, which the SSE dispatcher patches, so a
 * status change made from the CLI or by the daemon lands here on its own.
 *
 * The tab lives in the URL (like the panel itself), so a link can point at,
 * say, a session of a goal, and a reload stays where the user was. A session
 * (`?session=`) is a drill-down: it takes the panel over, goal header and tabs
 * included, with a link back to the goal.
 */

import { useQuery } from "@tanstack/react-query"
import { useSearchParams } from "react-router-dom"

import { ApiError, api, type GoalDto, qk, unwrap } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { StatusBadge } from "@/components/status-badge"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { GoalActions } from "./goal-actions"
import { GoalSessions, GoalSessionView } from "./goal-sessions"
import { GoalThread } from "./goal-thread"
import { goalQueryOptions } from "./queries"
import { GOAL_STATUS_META } from "./status"

const TABS = ["thread", "sessions"] as const
type Tab = (typeof TABS)[number]

export function GoalPanel({ goalId, onClose }: { goalId: string; onClose: () => void }) {
  const goal = useQuery(goalQueryOptions(goalId))
  const error = ApiError.is(goal.error) ? goal.error : null
  const [search, setSearch] = useSearchParams()
  const sessionId = search.get("session")

  /** The session the panel is drilled into; `null` goes back to the goal. */
  function selectSession(next: string | null) {
    const params = new URLSearchParams(search)
    if (next === null) params.delete("session")
    else params.set("session", next)
    setSearch(params, { replace: true })
  }

  // A selected session replaces the goal view entirely — the panel is that
  // session's now, and the way back is the link it carries. It is checked
  // before the goal query so a link into a session opens on it instead of
  // waiting for the goal it hangs off.
  if (sessionId) {
    return (
      <GoalSheet onClose={onClose}>
        <GoalSessionView
          goalId={goalId}
          goalTitle={goal.data?.title}
          sessionId={sessionId}
          onSelect={selectSession}
        />
      </GoalSheet>
    )
  }

  return (
    <GoalSheet onClose={onClose}>
      {error ? (
        <>
          <SheetTitle className="sr-only">Goal {goalId}</SheetTitle>
          <ErrorState
            title={error.status === 404 ? "No such goal" : "Could not load the goal"}
            error={error}
            // A goal that does not exist will not start existing on a retry.
            onRetry={error.status === 404 ? undefined : () => void goal.refetch()}
          />
        </>
      ) : null}

      {goal.isPending ? (
        <>
          <SheetTitle className="sr-only">Loading goal</SheetTitle>
          <Skeleton className="h-7 w-2/3" />
          <Skeleton className="h-40 w-full" />
        </>
      ) : null}

      {goal.data ? <GoalView goal={goal.data} onSelectSession={selectSession} /> : null}
    </GoalSheet>
  )
}

/** The panel itself, the same one whichever of the two views is inside it. */
function GoalSheet({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <Sheet open onOpenChange={(open) => open || onClose()}>
      {/* As wide as the task panel: the sessions tab holds a table. */}
      <SheetContent className="sm:max-w-3xl" aria-describedby={undefined}>
        {children}
      </SheetContent>
    </Sheet>
  )
}

function GoalView({
  goal,
  onSelectSession,
}: {
  goal: GoalDto
  /** Opens a session over the whole panel; owned by {@link GoalPanel}. */
  onSelectSession: (sessionId: string) => void
}) {
  const [search, setSearch] = useSearchParams()
  const tab = TABS.find((value) => value === search.get("tab")) ?? "thread"

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <>
      <SheetHeader>
        <div className="flex flex-wrap items-center gap-2">
          <SheetTitle>{goal.title}</SheetTitle>
          <StatusBadge
            label={GOAL_STATUS_META[goal.status].label}
            tone={GOAL_STATUS_META[goal.status].badge}
          />
        </div>
        <CopyableId value={goal.id} label="goal id" className="text-xs text-muted-foreground" />
      </SheetHeader>

      <GoalActions goal={goal} />

      {goal.description.trim() ? (
        <Card>
          <CardHeader>
            <CardTitle>Description</CardTitle>
          </CardHeader>
          <CardContent>
            <Markdown>{goal.description}</Markdown>
          </CardContent>
        </Card>
      ) : null}

      <GoalMetadata goal={goal} />

      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
        <TabsList>
          <TabsTrigger value="thread">Thread</TabsTrigger>
          <TabsTrigger value="sessions">Sessions</TabsTrigger>
        </TabsList>
        <TabsContent value="thread" className="pt-3">
          <GoalThread goalId={goal.id} />
        </TabsContent>
        <TabsContent value="sessions" className="pt-3">
          <GoalSessions goalId={goal.id} onSelect={onSelectSession} />
        </TabsContent>
      </Tabs>
    </>
  )
}

function GoalMetadata({ goal }: { goal: GoalDto }) {
  return (
    <Card className="h-fit">
      <CardHeader>
        <CardTitle>Details</CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 text-sm">
        <Detail label="Planner">
          <PlannerProfileName profileId={goal.planner_profile_id} />
        </Detail>
        <Detail label="Approvals">
          <span className="tabular-nums">{goal.required_approvals}</span>
        </Detail>
        <Detail label="Max tasks">
          <span className="tabular-nums">{goal.max_tasks ?? "unbounded"}</span>
        </Detail>
        <Detail label="Repositories">
          <ul className="flex flex-col gap-1">
            {goal.repos.map((repo) => (
              <li key={repo.id} className="min-w-0">
                <CopyableId
                  value={repo.path}
                  label="repository path"
                  className="block text-xs break-all"
                />
                <span className="font-mono text-xs text-muted-foreground">
                  base: {repo.base_branch}
                </span>
              </li>
            ))}
          </ul>
        </Detail>
        <Detail label="Created">
          <span title={formatAbsolute(goal.created_at)}>{formatRelative(goal.created_at)}</span>
        </Detail>
        <Detail label="Updated">
          <span title={formatAbsolute(goal.updated_at)}>{formatRelative(goal.updated_at)}</span>
        </Detail>
      </CardContent>
    </Card>
  )
}

function Detail({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[7rem_1fr] items-baseline gap-2">
      <span className="text-xs text-muted-foreground">{label}</span>
      <div className="min-w-0">{children}</div>
    </div>
  )
}

/** The goal only carries the profile id; the name is what a person recognises. */
function PlannerProfileName({ profileId }: { profileId: string }) {
  const profile = useQuery({
    queryKey: qk.profiles.detail(profileId),
    queryFn: () => unwrap(api().GET("/v1/profiles/{id}", { params: { path: { id: profileId } } })),
    staleTime: 5 * 60_000,
  })
  return (
    <span className="break-all">
      {profile.data?.name ?? (
        <CopyableId value={profileId} label="profile id" className="text-xs" />
      )}
    </span>
  )
}
