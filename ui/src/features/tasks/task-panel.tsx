/**
 * One task in full, in a side panel over whatever screen it was opened from —
 * the `task inspect` equivalent, plus everything hanging off it: its messages,
 * its transition log, its branch diff and the agents that ran it.
 *
 * The tab lives in the URL (like the panel itself) so a link can point at,
 * say, the diff of a task, and a reload stays where the user was. The sessions
 * tab keeps its selection there too, under `?session=` — and a selected
 * session is a drill-down: it takes the panel over, task header and tabs
 * included, with a link back to the task.
 *
 * The breadcrumb to its goal is the task's own `goal_id`, not where the panel
 * was opened from — every task has one, whatever opened this panel — and
 * clicking it replaces this panel with the goal's in the same pane.
 */

import { useQuery } from "@tanstack/react-query"
import { useRef } from "react"
import { useNavigate, useSearchParams } from "react-router-dom"

import type { TaskDto } from "@/api"
import { CopyableIdMenu } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { PanelHeader } from "@/components/panel-header"
import { PanelSheet } from "@/components/panel-sheet"
import { StatusBadge } from "@/components/status-badge"
import { TabCount } from "@/components/tab-count"
import { PaneBody, PaneHeader, PaneTitle } from "@/components/ui/docked-pane"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { When } from "@/components/when"
import { goalQueryOptions } from "@/features/goals/queries"
// Not the barrel: `@/features/sessions` re-exports the sessions table, which
// imports this feature back (see `sessions-list.tsx`).
import { sessionsQueryOptions } from "@/features/sessions/queries"
import { useFocusReturn } from "@/hooks/use-focus-return"
import { taskCopyEntries } from "@/lib/clipboard"
import { paths, usePanelSessionNavigation } from "@/routes/paths"

import { taskMessagesQueryOptions, taskQueryOptions } from "./queries"
import { StalledBadge } from "./stalled"
import { primaryStatus, subStatus, TASK_STATUS_META } from "./status"
import { TaskActions } from "./task-actions"
import { TaskDiff } from "./task-diff"
import { TaskFacts } from "./task-facts"
import { TaskHistory } from "./task-history"
import { TaskMessages } from "./task-messages"
import { TaskSessions, TaskSessionView } from "./task-sessions"

// Description leads the strip and is where the panel opens: it is what the
// task *is*, and the first thing to read on a task just landed on.
const TABS = ["description", "messages", "history", "diff", "sessions"] as const
type Tab = (typeof TABS)[number]

export function TaskPanel({ taskId, onClose }: { taskId: string; onClose: () => void }) {
  const [search, setSearch] = useSearchParams()
  const task = useQuery(taskQueryOptions(taskId))
  const tab = TABS.find((value) => value === search.get("tab")) ?? "description"
  const session = search.get("session") ?? undefined
  const selectSession = usePanelSessionNavigation()
  // Going back from a session hands focus to the row that opened it, which the
  // frame cannot do for us — nothing closed. See `useFocusReturn`.
  const panel = useRef<HTMLDivElement>(null)
  useFocusReturn(session ?? null, panel)
  // The two counted tabs, on the very keys their own tabs read, so the numbers
  // cost nothing beyond the first tab that is opened — and stay live with it,
  // since the dispatcher invalidates both lists. The sessions filter is the
  // one `TaskSessions` itself passes, or the tab's count and the tab's list
  // would be two cache entries and two requests.
  const sessions = useQuery(sessionsQueryOptions({ task: taskId }))
  const messages = useQuery(taskMessagesQueryOptions(taskId))

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <PanelSheet onClose={onClose} panelRef={panel}>
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
          <PaneHeader>
            <PaneTitle className="sr-only">Loading task</PaneTitle>
          </PaneHeader>
          <PaneBody>
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-28 w-full" />
            <Skeleton className="h-64 w-full" />
          </PaneBody>
        </>
      ) : task.error ? (
        <>
          <PaneHeader>
            <PaneTitle className="sr-only">Task {taskId}</PaneTitle>
          </PaneHeader>
          <PaneBody>
            <ErrorState
              title={`Could not load task ${taskId}`}
              error={task.error}
              onRetry={() => void task.refetch()}
            />
          </PaneBody>
        </>
      ) : (
        <>
          <TaskHeader task={task.data} />
          <PaneBody>
            <TaskFacts task={task.data} />

            <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
              <TabsList>
                <TabsTrigger value="description">Description</TabsTrigger>
                {/* The verdicts, not the rounds they are grouped into: a
                    round is how the tab is laid out, and a task around for the
                    third time has said more than three things about itself. */}
                <TabsTrigger value="messages">
                  Messages
                  <TabCount count={messages.data?.length} noun="message" />
                </TabsTrigger>
                <TabsTrigger value="history">History</TabsTrigger>
                <TabsTrigger value="diff">Diff</TabsTrigger>
                <TabsTrigger value="sessions">
                  Sessions
                  <TabCount count={sessions.data?.length} noun="session" />
                </TabsTrigger>
              </TabsList>
              <TabsContent value="description" className="pt-3">
                {task.data.description.trim() ? (
                  <Markdown>{task.data.description}</Markdown>
                ) : (
                  <EmptyState emphasis="quiet" title="This task has no description" />
                )}
              </TabsContent>
              <TabsContent value="messages" className="pt-3">
                <TaskMessages taskId={taskId} />
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
          </PaneBody>
        </>
      )}
    </PanelSheet>
  )
}

/**
 * The task's header: the breadcrumb back to its goal, the title and its
 * actions, and the dense line of its status, id and when it last moved.
 *
 * The breadcrumb is the task's own `goal_id`, not where the panel was opened
 * from — every task has one, whatever opened this panel — and clicking it
 * replaces this panel with the goal's in the same pane.
 */
function TaskHeader({ task }: { task: TaskDto }) {
  const goal = useQuery(goalQueryOptions(task.goal_id))
  const navigate = useNavigate()
  const status = TASK_STATUS_META[primaryStatus(task.status)]
  const sub = subStatus(task.status)
  return (
    <PanelHeader
      breadcrumb={{
        label: goal.data?.title ?? "Goal",
        // Replaces rather than pushes: the goal panel takes this one's place
        // in the pane, not a step stacked over it.
        onClick: () => navigate(paths.goal(task.goal_id), { replace: true }),
      }}
      title={task.title}
      actions={<TaskActions task={task} />}
      status={
        <>
          <StatusBadge label={status.label} tone={status.badge} hint={status.hint} />
          {sub && <StatusBadge label={sub.label} tone={sub.badge} hint={sub.hint} />}
          {task.stalled && <StalledBadge />}
        </>
      }
      id={<CopyableIdMenu value={task.id} label="task id" entries={taskCopyEntries(task.id)} />}
      stamps={
        <>
          <span>created</span>
          <When at={task.created_at} label="created" />
          <span aria-hidden="true">·</span>
          <span>updated</span>
          <When at={task.updated_at} label="updated" />
        </>
      }
    />
  )
}
