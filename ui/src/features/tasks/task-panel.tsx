/**
 * One task in full, in a side panel over whatever screen it was opened from —
 * the `task inspect` equivalent, plus everything hanging off it: its thread,
 * its reviews, its transition log, its branch diff and the agents that ran it.
 *
 * The tab lives in the URL (like the panel itself) so a link can point at,
 * say, the diff of a task, and a reload stays where the user was. The sessions
 * tab keeps its selection there too, under `?session=` — and a selected
 * session is a drill-down: it takes the panel over, task header and tabs
 * included, with a link back to the task.
 *
 * Opened from a goal's panel (`stackedOnGoal`), it is the second sheet of a
 * stack: narrower than the goal's, so that one keeps showing at its left, and
 * carrying the breadcrumb back to it.
 */

import { useQueries, useQuery } from "@tanstack/react-query"
import { ChevronRightIcon, GitBranchIcon, GitCommitHorizontalIcon } from "lucide-react"
import type { ReactNode } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { TaskDto } from "@/api"
import { CopyableId, CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { StatusBadge } from "@/components/status-badge"
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { goalQueryOptions } from "@/features/goals/queries"
import { taskCopyEntries } from "@/lib/copy-entries"
import { shortId } from "@/lib/ids"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { paths, usePanelSessionNavigation, useTaskPanelTo } from "@/routes/paths"
import { shortSha } from "./format"
import { taskQueryOptions } from "./queries"
import { StalledBadge } from "./stalled"
import { primaryStatus, subStatus, TASK_STATUS_META } from "./status"
import { TaskActions } from "./task-actions"
import { TaskConversation } from "./task-conversation"
import { TaskDiff } from "./task-diff"
import { TaskHistory } from "./task-history"
import { TaskReviews } from "./task-reviews"
import { TaskSessions, TaskSessionView } from "./task-sessions"

// Description leads the strip — it is what the task *is* — but the panel still
// opens on the conversation, which is what the user comes back for.
const TABS = ["description", "conversation", "reviews", "history", "diff", "sessions"] as const
type Tab = (typeof TABS)[number]

export function TaskPanel({
  taskId,
  onClose,
  stackedOnGoal,
}: {
  taskId: string
  onClose: () => void
  /** The goal whose panel this one opened over, when there is one under it. */
  stackedOnGoal?: string
}) {
  const [search, setSearch] = useSearchParams()
  const task = useQuery(taskQueryOptions(taskId))
  const tab = TABS.find((value) => value === search.get("tab")) ?? "conversation"
  const session = search.get("session") ?? undefined
  const selectSession = usePanelSessionNavigation()

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <Sheet open onOpenChange={(open) => open || onClose()}>
      <SheetContent
        // Stacked, it leaves the goal's sheet showing at its left and takes
        // the one darkening of the stack with it — the sheet underneath gives
        // its own up (see `GoalSheet`), so the screen is dimmed once and the
        // goal reads as being behind this.
        className={cn(
          "sm:max-w-3xl",
          stackedOnGoal && "w-[calc(100%-2.5rem)] shadow-2xl sm:max-w-2xl",
        )}
        overlay={stackedOnGoal ? { forceRender: true } : undefined}
      >
        {stackedOnGoal ? (
          <PanelBreadcrumb
            goalId={stackedOnGoal}
            taskTitle={task.data?.title}
            taskId={taskId}
            onOpenGoal={onClose}
          />
        ) : null}

        {/* A selected session replaces the task view entirely — the panel is
            that session's now, and the way back is the link it carries. It is
            checked before the task query so a link into a session opens on it
            instead of waiting for the task it hangs off. */}
        {session ? (
          <TaskSessionView
            taskId={taskId}
            taskTitle={task.data?.title}
            sessionId={session}
            onSelect={selectSession}
          />
        ) : task.isPending ? (
          <>
            <SheetTitle className="sr-only">Loading task</SheetTitle>
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-28 w-full" />
            <Skeleton className="h-64 w-full" />
          </>
        ) : task.error ? (
          <>
            <SheetTitle className="sr-only">Task {taskId}</SheetTitle>
            <ErrorState
              title={`Could not load task ${taskId}`}
              error={task.error}
              onRetry={() => void task.refetch()}
            />
          </>
        ) : (
          <>
            <TaskHeader task={task.data} showGoalLink={stackedOnGoal === undefined} />
            <TaskFacts task={task.data} />

            <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
              <TabsList>
                <TabsTrigger value="description">Description</TabsTrigger>
                <TabsTrigger value="conversation">Conversation</TabsTrigger>
                <TabsTrigger value="reviews">Reviews</TabsTrigger>
                <TabsTrigger value="history">History</TabsTrigger>
                <TabsTrigger value="diff">Diff</TabsTrigger>
                <TabsTrigger value="sessions">Sessions</TabsTrigger>
              </TabsList>
              <TabsContent value="description" className="pt-3">
                {task.data.description.trim() ? (
                  <Markdown>{task.data.description}</Markdown>
                ) : (
                  <EmptyState emphasis="quiet" title="This task has no description" />
                )}
              </TabsContent>
              <TabsContent value="conversation" className="pt-3">
                <TaskConversation taskId={taskId} />
              </TabsContent>
              <TabsContent value="reviews" className="pt-3">
                <TaskReviews taskId={taskId} />
              </TabsContent>
              <TabsContent value="history" className="pt-3">
                <TaskHistory taskId={taskId} />
              </TabsContent>
              <TabsContent value="diff" className="pt-3">
                <TaskDiff taskId={taskId} />
              </TabsContent>
              <TabsContent value="sessions" className="pt-3">
                <TaskSessions taskId={taskId} onSelect={selectSession} />
              </TabsContent>
            </Tabs>
          </>
        )}
      </SheetContent>
    </Sheet>
  )
}

/**
 * Where this panel sits: the goal it belongs to, then the task itself. The
 * goal segment drops this panel back onto the one underneath it, which is what
 * closing it does — the goal's sheet is already open behind this one.
 */
function PanelBreadcrumb({
  goalId,
  taskId,
  taskTitle,
  onOpenGoal,
}: {
  goalId: string
  taskId: string
  /** The task's own name, once it is loaded. */
  taskTitle?: string
  onOpenGoal: () => void
}) {
  const goal = useQuery(goalQueryOptions(goalId))
  return (
    <nav
      aria-label="Breadcrumb"
      // Clears the sheet's own close button, which floats over this row.
      className="flex min-w-0 items-center gap-1.5 pr-8 text-xs text-muted-foreground"
    >
      <button
        type="button"
        onClick={onOpenGoal}
        className="min-w-0 truncate underline-offset-3 hover:text-foreground hover:underline"
      >
        {goal.data?.title ?? "Goal"}
      </button>
      <ChevronRightIcon className="size-3 shrink-0" aria-hidden />
      <span className="min-w-0 truncate font-medium text-foreground" aria-current="page">
        {taskTitle ?? `task ${shortId(taskId)}`}
      </span>
    </nav>
  )
}

function TaskHeader({ task, showGoalLink }: { task: TaskDto; showGoalLink: boolean }) {
  const status = TASK_STATUS_META[primaryStatus(task.status)]
  const sub = subStatus(task.status)
  return (
    <SheetHeader>
      {/* Only when the goal is not already open behind this panel: stacked,
          the breadcrumb above is the way back to it. */}
      {showGoalLink ? (
        <Link
          to={paths.goal(task.goal_id)}
          className="w-fit text-xs text-muted-foreground underline-offset-3 hover:underline"
        >
          ← Open the goal
        </Link>
      ) : null}
      <div className="flex flex-wrap items-start gap-3">
        <SheetTitle>{task.title}</SheetTitle>
        <div className="ml-auto shrink-0">
          <TaskActions task={task} />
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <StatusBadge label={status.label} tone={status.badge} title={status.hint} />
        {sub && <StatusBadge label={sub.label} tone={sub.badge} title={sub.hint} />}
        {task.stalled && <StalledBadge />}
        <span className="text-muted-foreground">
          review round <span className="font-mono">{task.review_round}</span>
        </span>
        <CopyableIdMenu
          value={task.id}
          label="task id"
          entries={taskCopyEntries(task.id)}
          className="text-muted-foreground"
        />
        <span
          className="ml-auto text-muted-foreground"
          title={`created ${formatAbsolute(task.created_at)}`}
        >
          updated {formatRelative(task.updated_at)}
        </span>
      </div>
    </SheetHeader>
  )
}

function TaskFacts({ task }: { task: TaskDto }) {
  return (
    <dl className="grid gap-x-6 gap-y-3 rounded-lg border bg-card p-3 text-sm sm:grid-cols-2">
      <Fact label="Branch">
        <span className="flex items-center gap-1.5">
          <GitBranchIcon className="size-3.5 shrink-0 text-muted-foreground" />
          <CopyableId
            value={task.branch}
            label="branch"
            truncate="middle"
            className="text-xs"
          />
        </span>
      </Fact>
      <Fact label="Worktree">
        {task.worktree_path ? (
          <CopyableId value={task.worktree_path} label="worktree path" className="text-xs" />
        ) : (
          <Muted>not created yet</Muted>
        )}
      </Fact>
      <Fact label="Engineer">
        <CopyableId value={task.engineer_profile_id} label="profile id" className="text-xs" />
      </Fact>
      <Fact label="Reviewers">
        {task.reviewer_profile_ids.length > 0 ? (
          // Each id is its own click target; the separators are plain text so
          // the row still reads as the single list it was.
          <span className="text-xs">
            {task.reviewer_profile_ids.map((id, index) => (
              <span key={id}>
                {index > 0 ? ", " : null}
                <CopyableId value={id} label="profile id" className="text-xs" />
              </span>
            ))}
          </span>
        ) : (
          <Muted>none assigned</Muted>
        )}
      </Fact>
      <Fact label="Depends on">
        <Dependencies ids={task.depends_on} />
      </Fact>
      <Fact label="Merge commit">
        {task.merge_commit ? (
          <span className="flex items-center gap-1.5">
            <GitCommitHorizontalIcon className="size-3.5 shrink-0 text-muted-foreground" />
            <CopyableId
              value={task.merge_commit}
              display={shortSha}
              label="merge commit"
              className="text-xs"
            />
          </span>
        ) : (
          <Muted>not merged</Muted>
        )}
      </Fact>
    </dl>
  )
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 min-w-0">{children}</dd>
    </div>
  )
}

function Muted({ children }: { children: ReactNode }) {
  return <span className="text-xs text-muted-foreground">{children}</span>
}

/**
 * Dependencies are ids on the wire; each one is already in (or goes into) the
 * task cache, so they can be shown as what they actually are.
 */
function Dependencies({ ids }: { ids: string[] }) {
  const results = useQueries({ queries: ids.map((id) => taskQueryOptions(id)) })
  if (ids.length === 0) return <Muted>nothing — it can start as soon as it is scheduled</Muted>

  return (
    <ul className="flex flex-col gap-1">
      {ids.map((id, index) => {
        const dependency = results[index]?.data
        const status = dependency ? TASK_STATUS_META[primaryStatus(dependency.status)] : undefined
        return (
          <li key={id} className="flex min-w-0 items-center gap-1.5">
            {status && <span className={cn("size-1.5 shrink-0 rounded-full", status.dot)} />}
            <DependencyLink id={id} title={dependency?.title} />
            {status && (
              <span className="shrink-0 text-xs text-muted-foreground">{status.label}</span>
            )}
          </li>
        )
      })}
    </ul>
  )
}

/**
 * Swaps the open panel over to the dependency. It replaces rather than pushes:
 * the panel is still the same one entry of history, so closing it closes it
 * instead of stepping back through the tasks it was pointed at.
 */
function DependencyLink({ id, title }: { id: string; title?: string }) {
  const to = useTaskPanelTo(id)
  return (
    <Link
      to={to}
      replace
      className="truncate text-xs underline-offset-3 hover:underline"
      title={id}
    >
      {title ?? id}
    </Link>
  )
}
