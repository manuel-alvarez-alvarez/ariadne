/**
 * One goal in a side panel over the goals board: what it is, what it is
 * allowed to do, its tasks, its planner thread and the sessions it has run.
 * `ariadne goal inspect` plus `goal thread`.
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
import { PlusIcon } from "lucide-react"
import { type ReactNode, type RefObject, useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"

import { ApiError, type GoalDto } from "@/api"
import { CopyableId, CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Fact, FactList } from "@/components/fact-list"
import { Markdown } from "@/components/markdown"
import { PanelSheet } from "@/components/panel-sheet"
import { StatusBadge } from "@/components/status-badge"
import { UnreadBadge, useUnreadCount } from "@/components/thread-unread"
import { goalUsageRows, TokenFigure } from "@/components/token-figure"
import { Button } from "@/components/ui/button"
import { SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
import { ProfileSummary } from "@/features/profiles/profile-summary"
import { CreateTaskDialog } from "@/features/tasks/task-form-dialog"
import { useFocusReturn } from "@/hooks/use-focus-return"
import { goalCopyEntries } from "@/lib/clipboard"
import { paths, taskPanelTo, usePanelSessionNavigation } from "@/routes/paths"
import { GoalActions } from "./goal-actions"
import { GoalSessions, GoalSessionView } from "./goal-sessions"
import { GoalTasks } from "./goal-tasks"
import { GoalThread } from "./goal-thread"
import { goalMessagesQueryOptions, goalQueryOptions } from "./queries"
import { GOAL_STATUS_META, isStillPlanning, isTerminalGoalStatus } from "./status"

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
  // Going back from a session hands focus to the row that opened it, which the
  // dialog cannot do for us — nothing closed. Only while this panel is the one
  // on top, though: `sessionId` above also goes null when a task stacks over
  // it, and that is a sheet opening rather than a session being left. See
  // `useFocusReturn`.
  const panel = useRef<HTMLDivElement>(null)
  useFocusReturn(sessionId, panel, !stackedPanel)

  // A selected session replaces the goal view entirely — the panel is that
  // session's now, and the way back is the link it carries. It is checked
  // before the goal query so a link into a session opens on it instead of
  // waiting for the goal it hangs off.
  if (sessionId) {
    return (
      <GoalSheet goalId={goalId} onClose={onClose} stackedPanel={stackedPanel} panelRef={panel}>
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
    <GoalSheet goalId={goalId} onClose={onClose} stackedPanel={stackedPanel} panelRef={panel}>
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

      {goal.data ? (
        <GoalView goal={goal.data} onSelectSession={selectSession} onDeleted={onClose} />
      ) : null}
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
  goalId,
  onClose,
  stackedPanel,
  panelRef,
  children,
}: {
  /** Whose draft a dismissal has to ask about. */
  goalId: string
  onClose: () => void
  stackedPanel?: ReactNode
  /** The popup itself, for the focus this panel has to hand back by hand. */
  panelRef?: RefObject<HTMLDivElement | null>
  children: ReactNode
}) {
  return (
    // The draft the guard protects is the goal's own thread; a task stacked
    // over this panel guards its own, and Base UI only ever asks the topmost
    // sheet about a dismissal.
    <PanelSheet onClose={onClose} draftKey={`goal:${goalId}`}>
      {/* As wide as the task panel: the sessions tab holds a table. The panel
          on top is narrower, so this one keeps a strip showing at its left —
          the stack is a thing the user can see, and click back onto. */}
      <SheetContent
        ref={panelRef}
        className="sm:max-w-3xl"
        overlay={{ dim: !stackedPanel }}
        aria-describedby={undefined}
      >
        {children}
      </SheetContent>
      {stackedPanel}
    </PanelSheet>
  )
}

function GoalView({
  goal,
  onSelectSession,
  onDeleted,
}: {
  goal: GoalDto
  /** Opens a session over the whole panel; owned by {@link GoalPanel}. */
  onSelectSession: (sessionId: string) => void
  /** Closes the panel once the goal it is showing has been deleted. */
  onDeleted: () => void
}) {
  const [search, setSearch] = useSearchParams()
  const [newTaskOpen, setNewTaskOpen] = useState(false)
  const tab = TABS.find((value) => value === search.get("tab")) ?? "description"
  // Read whichever tab is showing: the Thread trigger counts what has been
  // said since the reader last had the thread itself open, which is a thing
  // the panel has to know before they go back to it.
  const messages = useQuery(goalMessagesQueryOptions(goal.id))
  const unread = useUnreadCount(`goal:${goal.id}`, messages.data)

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  // The daemon takes a task in any goal state, but only a live goal does
  // anything with one: while planning it joins the plan, while active the
  // scheduler picks it up once its dependencies merge. On a terminal goal it
  // would only ever sit in pending, so the button goes away with the goal.
  const canCreateTask = !isTerminalGoalStatus(goal.status)

  return (
    <>
      <SheetHeader>
        {/* What can be done to the goal sits at the end of the title row, the
            same slot the task and session panels put their actions in. */}
        <div className="flex flex-wrap items-center gap-3">
          <SheetTitle>{goal.title}</SheetTitle>
          <StatusBadge
            box="badge"
            label={GOAL_STATUS_META[goal.status].label}
            tone={GOAL_STATUS_META[goal.status].badge}
          />
          <div className="ml-auto flex shrink-0 items-center gap-2">
            {canCreateTask ? (
              <Button variant="outline" size="sm" onClick={() => setNewTaskOpen(true)}>
                <PlusIcon />
                New task
              </Button>
            ) : null}
            {/* Deleting takes the goal out from under this panel, so the
                panel's own close is what the action ends on. */}
            <GoalActions goal={goal} onDeleted={onDeleted} />
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
          <TabsTrigger value="thread">
            Thread
            <UnreadBadge count={unread} />
          </TabsTrigger>
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
          <GoalTasks
            goalId={goal.id}
            awaitingPlan={isStillPlanning(goal.status)}
            onNewTask={canCreateTask ? () => setNewTaskOpen(true) : undefined}
          />
        </TabsContent>
        <TabsContent value="thread" className="pt-3">
          <GoalThread goalId={goal.id} />
        </TabsContent>
        <TabsContent value="sessions" className="pt-3">
          <GoalSessions goalId={goal.id} onSelect={onSelectSession} />
        </TabsContent>
      </Tabs>

      <CreateTaskDialog
        goal={goal}
        open={newTaskOpen}
        onOpenChange={setNewTaskOpen}
        // Opening the new task's panel is the same gesture as opening it from
        // a lane: `?task=` stacks it over this goal (see `detail-panels.tsx`),
        // pushed so Back lands here.
        onCreated={(task) => setSearch(taskPanelTo(search, task.id).search)}
      />
    </>
  )
}

/**
 * What the goal is allowed to do and what it has cost, always on show.
 *
 * Three columns where there is room, like the session panel's facts, and six
 * short facts to fill them: two whole rows at three columns and three whole
 * rows at two, which the five facts it started with did not manage — one of
 * them left a hole in the middle of the card. The repositories are the seventh
 * and take a row of their own at the end.
 */
function GoalMetadata({ goal }: { goal: GoalDto }) {
  return (
    <FactList>
      <Fact label="Planner">
        {/* The goal's pin: what the planner runs on, which a later edit to its
            profile leaves alone. */}
        <ProfileSummary profileId={goal.planner_profile_id} model={goal.model} />
      </Fact>
      <Fact label="Approvals">
        <span className="tabular-nums">{goal.required_approvals}</span>
      </Fact>
      <Fact label="Max tasks">
        <span className="tabular-nums">{goal.max_tasks ?? "unbounded"}</span>
      </Fact>
      <Fact label="Created">
        <When at={goal.created_at} label="created" />
      </Fact>
      <Fact label="Updated">
        <When at={goal.updated_at} label="updated" />
      </Fact>
      <Fact label="Tokens">
        {/* Every session of the goal, its planner's included, with the hint
            breaking the same total down by the role that spent it. */}
        <TokenFigure
          usage={goal.usage.total}
          rows={goalUsageRows(goal.usage)}
          className="text-xs"
        />
      </Fact>
      <Fact label="Repositories" className="sm:col-span-2 lg:col-span-3">
        {/* One line per repository, whatever each one carries: the base branch
            used to drop to a line of its own under the path and the
            description to a third, so a goal on two repositories read as an
            uneven list of three, four or five lines with no shape to it. The
            path is cut short before the branch beside it is, since it is the
            branch that says what the task worktrees are cut from. */}
        <ul className="flex flex-col gap-1">
          {goal.repos.map((repo) => (
            <li key={repo.id} className="flex min-w-0 items-baseline gap-1.5">
              {/* The path is the repository's name, so it is also the way to
                  its registration — where the base branch beside it and its
                  description are edited (the rows there do not expand, so the
                  screen itself is as far as a link can point). */}
              <CopyableId
                value={repo.path}
                label="repository path"
                truncate="middle"
                to={paths.repositories()}
                className="text-xs"
              />
              <span className="shrink-0 text-xs text-muted-foreground">
                · base <span className="font-mono">{repo.base_branch}</span>
              </span>
            </li>
          ))}
        </ul>
      </Fact>
    </FactList>
  )
}
