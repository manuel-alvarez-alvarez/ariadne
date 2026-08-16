/**
 * One task in full — the `task inspect` equivalent, plus everything hanging off
 * it: its thread, its reviews, its transition log and its branch diff.
 *
 * The tab lives in the URL so a link can point at, say, the diff of a task, and
 * a reload stays where the user was.
 */

import { useQueries, useQuery } from "@tanstack/react-query"
import { GitBranchIcon, GitCommitHorizontalIcon, TriangleAlertIcon } from "lucide-react"
import type { ReactNode } from "react"
import { Link, useParams, useSearchParams } from "react-router-dom"

import type { TaskDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { cn } from "@/lib/utils"
import { paths } from "@/routes/paths"
import { describeError, formatAbsolute, formatRelative, shortSha } from "./format"
import { Markdown } from "./markdown"
import { taskQueryOptions } from "./queries"
import { TASK_STATUS_META } from "./status"
import { TaskActions } from "./task-actions"
import { TaskConversation } from "./task-conversation"
import { TaskDiff } from "./task-diff"
import { TaskHistory } from "./task-history"
import { TaskReviews } from "./task-reviews"

const TABS = ["conversation", "reviews", "history", "diff"] as const
type Tab = (typeof TABS)[number]

export function TaskDetailPage() {
  const { taskId = "" } = useParams<{ taskId: string }>()
  const [search, setSearch] = useSearchParams()
  const task = useQuery(taskQueryOptions(taskId))
  const tab = TABS.find((value) => value === search.get("tab")) ?? "conversation"

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  if (task.isPending) {
    return (
      <div className="mx-auto flex max-w-5xl flex-col gap-4">
        <Skeleton className="h-7 w-2/3" />
        <Skeleton className="h-28 w-full" />
        <Skeleton className="h-64 w-full" />
      </div>
    )
  }

  if (task.error) {
    return (
      <Alert variant="destructive" className="mx-auto max-w-2xl">
        <AlertTitle>Could not load task {taskId}</AlertTitle>
        <AlertDescription>{describeError(task.error)}</AlertDescription>
      </Alert>
    )
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-5">
      <TaskHeader task={task.data} />
      <TaskFacts task={task.data} />

      {task.data.description.trim().length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">Description</CardTitle>
          </CardHeader>
          <CardContent>
            <Markdown>{task.data.description}</Markdown>
          </CardContent>
        </Card>
      )}

      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
        <TabsList>
          <TabsTrigger value="conversation">Conversation</TabsTrigger>
          <TabsTrigger value="reviews">Reviews</TabsTrigger>
          <TabsTrigger value="history">History</TabsTrigger>
          <TabsTrigger value="diff">Diff</TabsTrigger>
        </TabsList>
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
      </Tabs>
    </div>
  )
}

function TaskHeader({ task }: { task: TaskDto }) {
  const status = TASK_STATUS_META[task.status]
  return (
    <header className="space-y-2">
      <Link
        to={paths.goal(task.goal_id)}
        className="text-xs text-muted-foreground underline-offset-3 hover:underline"
      >
        ← Back to the goal
      </Link>
      <div className="flex flex-wrap items-start gap-3">
        <h1 className="font-heading text-lg leading-tight font-semibold">{task.title}</h1>
        <div className="ml-auto shrink-0">
          <TaskActions task={task} />
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span
          className={cn("rounded-full px-2 py-0.5 font-medium", status.badge)}
          title={status.hint}
        >
          {status.label}
        </span>
        {task.stalled && (
          <span
            className="flex items-center gap-1 rounded-full bg-amber-500/12 px-2 py-0.5 font-medium text-amber-700 dark:bg-amber-400/15 dark:text-amber-300"
            title="The agent went idle without advancing the task."
          >
            <TriangleAlertIcon className="size-3" />
            stalled
          </span>
        )}
        <span className="text-muted-foreground">
          review round <span className="font-mono">{task.review_round}</span>
        </span>
        <span className="font-mono text-muted-foreground" title={task.id}>
          {task.id}
        </span>
        <span
          className="ml-auto text-muted-foreground"
          title={`created ${formatAbsolute(task.created_at)}`}
        >
          updated {formatRelative(task.updated_at)}
        </span>
      </div>
    </header>
  )
}

function TaskFacts({ task }: { task: TaskDto }) {
  return (
    <dl className="grid gap-x-6 gap-y-3 rounded-lg border bg-card p-3 text-sm sm:grid-cols-2">
      <Fact label="Branch">
        <span className="flex items-center gap-1.5 font-mono text-xs">
          <GitBranchIcon className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="truncate" title={task.branch}>
            {task.branch}
          </span>
        </span>
      </Fact>
      <Fact label="Worktree">
        {task.worktree_path ? (
          <span className="block truncate font-mono text-xs" title={task.worktree_path}>
            {task.worktree_path}
          </span>
        ) : (
          <Muted>not created yet</Muted>
        )}
      </Fact>
      <Fact label="Engineer">
        <span className="font-mono text-xs">{task.engineer_profile_id}</span>
      </Fact>
      <Fact label="Reviewers">
        {task.reviewer_profile_ids.length > 0 ? (
          <span className="font-mono text-xs">{task.reviewer_profile_ids.join(", ")}</span>
        ) : (
          <Muted>none assigned</Muted>
        )}
      </Fact>
      <Fact label="Depends on">
        <Dependencies ids={task.depends_on} />
      </Fact>
      <Fact label="Merge commit">
        {task.merge_commit ? (
          <span className="flex items-center gap-1.5 font-mono text-xs" title={task.merge_commit}>
            <GitCommitHorizontalIcon className="size-3.5 shrink-0 text-muted-foreground" />
            {shortSha(task.merge_commit)}
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
        const status = dependency ? TASK_STATUS_META[dependency.status] : undefined
        return (
          <li key={id} className="flex min-w-0 items-center gap-1.5">
            {status && <span className={cn("size-1.5 shrink-0 rounded-full", status.dot)} />}
            <Link
              to={paths.task(id)}
              className="truncate text-xs underline-offset-3 hover:underline"
              title={id}
            >
              {dependency?.title ?? id}
            </Link>
            {status && (
              <span className="shrink-0 text-xs text-muted-foreground">{status.label}</span>
            )}
          </li>
        )
      })}
    </ul>
  )
}
