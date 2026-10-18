/**
 * One goal in a side panel over the goals board: what it is, what it is
 * allowed to do, its tasks and the sessions it has run. `ariadne goal
 * inspect`.
 *
 * Everything reads the query cache, which the SSE dispatcher patches, so a
 * status change made from the CLI or by the daemon lands here on its own.
 *
 * The tab lives in the URL (like the panel itself), so a link can point at,
 * say, a session of a goal, and a reload stays where the user was. A session
 * (`?session=`) is a drill-down: it takes the panel over, goal header and tabs
 * included, with a link back to the goal.
 *
 * A task opened from here replaces this panel in the pane — see
 * `task-card.tsx` and `task-panel.tsx`'s breadcrumb for the way back.
 */

import { useQuery } from "@tanstack/react-query"
import { PlusIcon } from "lucide-react"
import { useRef, useState } from "react"
import { useSearchParams } from "react-router-dom"

import { ApiError, type GoalDto } from "@/api"
import { CopyableId, CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Fact, FactList } from "@/components/fact-list"
import { Markdown } from "@/components/markdown"
import { PanelHeader } from "@/components/panel-header"
import { PanelSheet } from "@/components/panel-sheet"
import { StatusBadge } from "@/components/status-badge"
import { TabCount } from "@/components/tab-count"
import { goalUsageRows, TokenFigure } from "@/components/token-figure"
import { Button } from "@/components/ui/button"
import { PaneBody, PaneHeader, PaneTitle } from "@/components/ui/docked-pane"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
import { ModelPin } from "@/features/models/model-pin"
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { taskListQueryOptions } from "@/features/tasks"
import { CreateTaskDialog } from "@/features/tasks/task-form-dialog"
import { useFocusReturn } from "@/hooks/use-focus-return"
import { goalCopyEntries } from "@/lib/clipboard"
import { folderName } from "@/lib/format"
import { paths, taskPanelTo, usePanelSessionNavigation } from "@/routes/paths"
import { GoalActions } from "./goal-actions"
import { GoalSessions, GoalSessionView } from "./goal-sessions"
import { GoalTasks } from "./goal-tasks"
import { goalQueryOptions } from "./queries"
import { GOAL_STATUS_META, isTerminalGoalStatus } from "./status"

// Description leads the strip — it is what the goal *is* — but the panel opens
// on the tasks, which is what the goal comes down to, whatever its status.
const TABS = ["description", "tasks", "sessions"] as const
type Tab = (typeof TABS)[number]

/** Where the panel opens when the URL does not say: the tasks, always. */
const DEFAULT_TAB: Tab = "tasks"

export function GoalPanel({ goalId, onClose }: { goalId: string; onClose: () => void }) {
  const goal = useQuery(goalQueryOptions(goalId))
  const error = ApiError.is(goal.error) ? goal.error : null
  const [search] = useSearchParams()
  const selectSession = usePanelSessionNavigation()
  const sessionId = search.get("session")
  // Going back from a session hands focus to the row that opened it, which the
  // frame cannot do for us — nothing closed. See `useFocusReturn`.
  const panel = useRef<HTMLDivElement>(null)
  useFocusReturn(sessionId, panel)

  // A selected session replaces the goal view entirely — the panel is that
  // session's now, and the way back is the link it carries. It is checked
  // before the goal query so a link into a session opens on it instead of
  // waiting for the goal it hangs off.
  if (sessionId) {
    return (
      <PanelSheet onClose={onClose} panelRef={panel}>
        <GoalSessionView
          goalId={goalId}
          goalTitle={goal.data?.title}
          sessionId={sessionId}
          onSelect={selectSession}
        />
      </PanelSheet>
    )
  }

  return (
    <PanelSheet onClose={onClose} panelRef={panel}>
      {goal.data ? (
        <GoalView
          goal={goal.data}
          error={error}
          onRetry={() => void goal.refetch()}
          onSelectSession={selectSession}
          onDeleted={onClose}
        />
      ) : (
        <>
          <PaneHeader>
            <PaneTitle className="sr-only">{error ? `Goal ${goalId}` : "Loading goal"}</PaneTitle>
          </PaneHeader>
          <PaneBody>
            {error ? (
              <ErrorState
                showIcon
                title={error.status === 404 ? "No such goal" : "Could not load goal"}
                error={error}
                // A goal that does not exist will not start existing on a retry.
                onRetry={error.status === 404 ? undefined : () => void goal.refetch()}
              />
            ) : (
              <>
                <Skeleton className="h-7 w-2/3" />
                <Skeleton className="h-40 w-full" />
              </>
            )}
          </PaneBody>
        </>
      )}
    </PanelSheet>
  )
}

function GoalView({
  goal,
  error,
  onRetry,
  onSelectSession,
  onDeleted,
}: {
  goal: GoalDto
  /** A refetch that failed with this goal already cached; the stale data below is still shown. */
  error: ApiError | null
  onRetry: () => void
  /** Opens a session over the whole panel; owned by {@link GoalPanel}. */
  onSelectSession: (sessionId: string) => void
  /** Closes the panel once the goal it is showing has been deleted. */
  onDeleted: () => void
}) {
  const [search, setSearch] = useSearchParams()
  const [newTaskOpen, setNewTaskOpen] = useState(false)
  const tab = TABS.find((value) => value === search.get("tab")) ?? DEFAULT_TAB
  // The two counted tabs, on the very keys their own tabs read, so the numbers
  // cost nothing beyond the first tab that is opened — and stay live with it,
  // since the dispatcher invalidates both lists.
  const tasks = useQuery(taskListQueryOptions({ goal: goal.id }))
  const sessions = useQuery(sessionsQueryOptions({ goal: goal.id, seat: "orchestrator" }))

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
      <PanelHeader
        title={goal.title}
        actions={
          <div className="flex items-center gap-2">
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
        }
        status={
          <StatusBadge
            box="badge"
            label={GOAL_STATUS_META[goal.status].label}
            tone={GOAL_STATUS_META[goal.status].badge}
          />
        }
        id={<CopyableIdMenu value={goal.id} label="goal id" entries={goalCopyEntries(goal.id)} />}
        stamps={
          <>
            <span>created</span>
            <When at={goal.created_at} label="created" />
            <span aria-hidden="true">·</span>
            <span>updated</span>
            <When at={goal.updated_at} label="updated" />
          </>
        }
      />

      <PaneBody>
        {/* A refetch can fail with the goal already cached — the dashboard
            keeps polling it — and the stale data is still worth showing, with
            one notice above it rather than losing the panel to the error. */}
        {error ? (
          <ErrorState title="Could not refresh this goal" error={error} onRetry={onRetry} />
        ) : null}

        <GoalMetadata goal={goal} />

        <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
          <TabsList>
            <TabsTrigger value="description">Description</TabsTrigger>
            <TabsTrigger value="tasks">
              Tasks
              <TabCount count={tasks.data?.length} noun="task" />
            </TabsTrigger>
            {/* The goal's own sessions: its orchestrator, once per resume or
              restart. The sessions its tasks have run are each task panel's,
              which is why the count here is a small number. */}
            <TabsTrigger value="sessions">
              Sessions
              <TabCount count={sessions.data?.length} noun="session" />
            </TabsTrigger>
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
              steps={goal.steps}
              onNewTask={canCreateTask ? () => setNewTaskOpen(true) : undefined}
            />
          </TabsContent>
          <TabsContent value="sessions" className="pt-3">
            <GoalSessions goalId={goal.id} onSelect={onSelectSession} />
          </TabsContent>
        </Tabs>
      </PaneBody>

      <CreateTaskDialog
        goal={goal}
        open={newTaskOpen}
        onOpenChange={setNewTaskOpen}
        // Opening the new task's panel is the same gesture as opening it from
        // a lane: `?task=` replaces this goal's panel (see `detail-panels.tsx`).
        // Replaced rather than pushed — the task's panel takes this goal's
        // place in the pane, not a step stacked over it.
        onCreated={(task) => setSearch(taskPanelTo(search, task.id).search, { replace: true })}
      />
    </>
  )
}

/**
 * What the goal is allowed to do and what it has cost, in the dense strip
 * every panel opens on — the stamps that say when are on the meta line above
 * this, beside the id, rather than a fact apiece down here.
 */
function GoalMetadata({ goal }: { goal: GoalDto }) {
  return (
    <FactList dense>
      <Fact label="Orchestrator">
        {/* The goal's pin: what its orchestrator runs on, frozen when the goal
            was created. */}
        {goal.orchestrated ? (
          <ModelPin model={goal.model} effort={goal.effort} mode="line" />
        ) : (
          <span>No orchestrator</span>
        )}
      </Fact>
      <Fact label="Tokens">
        {/* Every session of the goal, its orchestrator's included, with the hint
            breaking the same total down by the seat that spent it. */}
        <TokenFigure usage={goal.usage.total} rows={goalUsageRows(goal.usage)} />
      </Fact>
      {goal.issue_url && (
        <Fact label="Issue">
          <a href={goal.issue_url} target="_blank" rel="noreferrer">
            {goal.issue_url}
          </a>
        </Fact>
      )}
      <Fact label="Repositories" className="sm:col-span-3 lg:col-span-4">
        {/* Named by its folder rather than its full path — the path is only
            worth the room a tooltip gives it — with the base branch beside it
            in brackets, and a feature-branch goal's own branch after that once
            the plan has cut one. No separator shows where there is nothing on
            the other side of it. */}
        <ul className="flex flex-col gap-1">
          {goal.repos.map((repo) => (
            <li key={repo.id} className="min-w-0">
              <CopyableId
                value={repo.path}
                display={() => `${folderName(repo.path)} [${repo.base_branch}]`}
                label="repository path"
                to={paths.repositories()}
              />
            </li>
          ))}
        </ul>
      </Fact>
    </FactList>
  )
}
