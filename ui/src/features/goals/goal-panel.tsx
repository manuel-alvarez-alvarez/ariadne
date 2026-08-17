/**
 * One goal in a side panel over the goals board: what it is, what it is
 * allowed to do, its tasks, its planner thread and the sessions it has run.
 * `ariadne goal inspect` plus `goal messages`.
 *
 * Everything reads the query cache, which the SSE dispatcher patches, so a
 * status change made from the CLI or by the daemon lands here on its own.
 *
 * The tab lives in the URL (like the panel itself), so a link can point at,
 * say, a session of a goal, and a reload stays where the user was. A session
 * (`?session=`) is a drill-down: it takes the panel over, goal header and tabs
 * included, with a link back to the goal.
 *
 * A task opened from here stacks *inside* this panel's dialog (`stackedPanel`)
 * rather than beside it — see {@link GoalSheet}.
 */

import { useQuery } from "@tanstack/react-query"
import type { ReactNode } from "react"
import { useSearchParams } from "react-router-dom"

import { ApiError, type GoalDto } from "@/api"
import { CopyableId, CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { StatusBadge } from "@/components/status-badge"
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ProfileName } from "@/features/profiles/profile-name"
import { goalCopyEntries } from "@/lib/copy-entries"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { usePanelSessionNavigation } from "@/routes/paths"
import { GoalActions } from "./goal-actions"
import { GoalSessions, GoalSessionView } from "./goal-sessions"
import { GoalTasks } from "./goal-tasks"
import { GoalThread } from "./goal-thread"
import { goalQueryOptions } from "./queries"
import { GOAL_STATUS_META } from "./status"

// Description leads the strip — it is what the goal *is* — but the panel still
// opens on the tasks, which are what a goal comes down to.
const TABS = ["description", "tasks", "thread", "sessions"] as const
type Tab = (typeof TABS)[number]

export function GoalPanel({
  goalId,
  onClose,
  stackedPanel,
}: {
  goalId: string
  onClose: () => void
  /**
   * The task panel, when one is open over this goal. It is rendered inside
   * this panel's dialog so it stacks on it instead of replacing it.
   */
  stackedPanel?: ReactNode
}) {
  const goal = useQuery(goalQueryOptions(goalId))
  const error = ApiError.is(goal.error) ? goal.error : null
  const [search] = useSearchParams()
  const selectSession = usePanelSessionNavigation()
  // `tab` and `session` belong to whichever panel is on top: while a task is
  // stacked over this one they are its, and this goal shows its own default.
  const sessionId = stackedPanel ? null : search.get("session")

  // A selected session replaces the goal view entirely — the panel is that
  // session's now, and the way back is the link it carries. It is checked
  // before the goal query so a link into a session opens on it instead of
  // waiting for the goal it hangs off.
  if (sessionId) {
    return (
      <GoalSheet onClose={onClose} stackedPanel={stackedPanel}>
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
    <GoalSheet onClose={onClose} stackedPanel={stackedPanel}>
      {error ? (
        <>
          <SheetTitle className="sr-only">Goal {goalId}</SheetTitle>
          <ErrorState
            showIcon
            title={error.status === 404 ? "No such goal" : "Could not load goal"}
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

/**
 * The panel itself, the same one whichever of the two views is inside it —
 * and the dialog the task panel opens *inside*.
 *
 * Nesting it there rather than mounting it alongside is what makes the stack
 * behave like one: Base UI then gives the nested sheet no backdrop of its own
 * (so the screen is darkened once), lets only the topmost sheet answer Escape
 * and an outside press, and tells this popup it has a dialog over it through
 * `data-nested-dialog-open`.
 */
function GoalSheet({
  onClose,
  stackedPanel,
  children,
}: {
  onClose: () => void
  stackedPanel?: ReactNode
  children: ReactNode
}) {
  return (
    <Sheet open onOpenChange={(open) => open || onClose()}>
      {/* As wide as the task panel: the sessions tab holds a table. The panel
          on top is narrower, so this one keeps a strip showing at its left —
          the stack is a thing the user can see, and click back onto. */}
      <SheetContent
        className="sm:max-w-3xl"
        overlay={{ dim: !stackedPanel }}
        aria-describedby={undefined}
      >
        {children}
      </SheetContent>
      {stackedPanel}
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
  // Tasks by default, whatever the URL says if it says something else: what a
  // goal comes down to is its tasks, and they are otherwise on the board this
  // panel covers.
  const tab = TABS.find((value) => value === search.get("tab")) ?? "tasks"

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <>
      <SheetHeader>
        {/* What can be done to the goal sits at the end of the title row, the
            same slot the task and session panels put their actions in. */}
        <div className="flex flex-wrap items-start gap-3">
          <SheetTitle>{goal.title}</SheetTitle>
          <StatusBadge
            box="badge"
            label={GOAL_STATUS_META[goal.status].label}
            tone={GOAL_STATUS_META[goal.status].badge}
          />
          <div className="ml-auto shrink-0">
            <GoalActions goal={goal} />
          </div>
        </div>
        <CopyableIdMenu
          value={goal.id}
          label="goal id"
          entries={goalCopyEntries(goal.id)}
          className="text-xs text-muted-foreground"
        />
      </SheetHeader>

      <GoalMetadata goal={goal} />

      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
        <TabsList>
          <TabsTrigger value="description">Description</TabsTrigger>
          <TabsTrigger value="tasks">Tasks</TabsTrigger>
          <TabsTrigger value="thread">Thread</TabsTrigger>
          <TabsTrigger value="sessions">Sessions</TabsTrigger>
        </TabsList>
        <TabsContent value="description" className="pt-3">
          {goal.description.trim() ? (
            <Markdown>{goal.description}</Markdown>
          ) : (
            <EmptyState emphasis="quiet" title="This goal has no description" />
          )}
        </TabsContent>
        <TabsContent value="tasks" className="pt-3">
          <GoalTasks goalId={goal.id} />
        </TabsContent>
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

/**
 * What the goal is allowed to do, always on show.
 *
 * Three columns where there is room, like the session panel's Details card,
 * and six facts to fill them: at every width the grid comes out whole, which
 * the five short facts plus a full-width row of repositories did not — it left
 * a hole in the middle of the card. The repositories take the last cell and
 * wrap inside it, which is what {@link CopyableId.wrap} is for.
 */
function GoalMetadata({ goal }: { goal: GoalDto }) {
  return (
    <dl className="grid gap-x-6 gap-y-3 rounded-lg border bg-card p-3 text-sm sm:grid-cols-2 lg:grid-cols-3">
      <Detail label="Planner">
        <ProfileName profileId={goal.planner_profile_id} className="text-sm" />
      </Detail>
      <Detail label="Approvals">
        <span className="tabular-nums">{goal.required_approvals}</span>
      </Detail>
      <Detail label="Max tasks">
        <span className="tabular-nums">{goal.max_tasks ?? "unbounded"}</span>
      </Detail>
      <Detail label="Created">
        <span title={formatAbsolute(goal.created_at)}>{formatRelative(goal.created_at)}</span>
      </Detail>
      <Detail label="Updated">
        <span title={formatAbsolute(goal.updated_at)}>{formatRelative(goal.updated_at)}</span>
      </Detail>
      <Detail label="Repositories">
        <ul className="flex flex-col gap-1">
          {goal.repos.map((repo) => (
            <li key={repo.id} className="min-w-0">
              <CopyableId value={repo.path} label="repository path" wrap className="text-xs" />
              <span className="font-mono text-xs text-muted-foreground">
                base: {repo.base_branch}
              </span>
            </li>
          ))}
        </ul>
      </Detail>
    </dl>
  )
}

function Detail({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0 space-y-0.5">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="min-w-0">{children}</dd>
    </div>
  )
}
