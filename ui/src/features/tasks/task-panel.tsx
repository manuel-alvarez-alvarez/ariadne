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

import { useQuery } from "@tanstack/react-query"
import { ChevronRightIcon } from "lucide-react"
import { useRef } from "react"
import { Link, useSearchParams } from "react-router-dom"

import type { TaskDto } from "@/api"
import { CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { PanelSheet } from "@/components/panel-sheet"
import { StatusBadge } from "@/components/status-badge"
import { UnreadBadge, useUnreadCount } from "@/components/thread-unread"
import { SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When, WhenDetail } from "@/components/when"
import { goalQueryOptions } from "@/features/goals/queries"
import { useFocusReturn } from "@/hooks/use-focus-return"
import { taskCopyEntries } from "@/lib/clipboard"
import { cn, shortId } from "@/lib/format"
import { paths, usePanelSessionNavigation } from "@/routes/paths"

import { taskMessagesQueryOptions, taskQueryOptions } from "./queries"
import { StalledBadge } from "./stalled"
import { primaryStatus, subStatus, TASK_STATUS_META } from "./status"
import { TaskActions } from "./task-actions"
import { TaskConversation } from "./task-conversation"
import { TaskDiff } from "./task-diff"
import { TaskFacts } from "./task-facts"
import { TaskHistory } from "./task-history"
import { TaskReviews } from "./task-reviews"
import { TaskSessions, TaskSessionView } from "./task-sessions"

// Description leads the strip and is where the panel opens: it is what the
// task *is*, and the first thing to read on a task just landed on.
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
  const tab = TABS.find((value) => value === search.get("tab")) ?? "description"
  const session = search.get("session") ?? undefined
  const selectSession = usePanelSessionNavigation()
  // Going back from a session hands focus to the row that opened it, which the
  // dialog cannot do for us — nothing closed. See `useFocusReturn`.
  const panel = useRef<HTMLDivElement>(null)
  useFocusReturn(session ?? null, panel)
  // Read whichever tab is showing: the Conversation trigger counts what has
  // been said since the reader last had the thread itself open, which is a
  // thing the panel has to know before they go back to it.
  const messages = useQuery(taskMessagesQueryOptions(taskId))
  const unread = useUnreadCount(`task:${taskId}`, messages.data)

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <PanelSheet onClose={onClose} draftKey={`task:${taskId}`}>
      <SheetContent
        ref={panel}
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
                <TabsTrigger value="conversation">
                  Conversation
                  <UnreadBadge count={unread} />
                </TabsTrigger>
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
                <TaskSessions taskId={task.data.id} onSelect={selectSession} />
              </TabsContent>
            </Tabs>
          </>
        )}
      </SheetContent>
    </PanelSheet>
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
        // The ring every other control in the app wears: this one is the first
        // thing focused when a deep link opens the panel, and it was showing
        // the browser's own outline there.
        className="min-w-0 truncate rounded-xs underline-offset-3 outline-none hover:text-foreground hover:underline focus-visible:ring-3 focus-visible:ring-ring/50"
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
      {/* The actions stay on the title row whatever the task's status, which
          is what the title shrinking rather than wrapping the row buys: a
          `Cancel task` on its own line under the title read as a second row of
          header, and which line it landed on came down to how long the title
          was and how many buttons the status offers. */}
      <div className="flex items-start gap-3">
        <SheetTitle className="min-w-0 flex-1">{task.title}</SheetTitle>
        <div className="shrink-0">
          <TaskActions task={task} />
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <StatusBadge label={status.label} tone={status.badge} hint={status.hint} />
        {sub && <StatusBadge label={sub.label} tone={sub.badge} hint={sub.hint} />}
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
        <span className="ml-auto text-muted-foreground">
          updated{" "}
          <When
            at={task.updated_at}
            label="updated"
            detail={<WhenDetail label="created" at={task.created_at} />}
          />
        </span>
      </div>
    </SheetHeader>
  )
}
