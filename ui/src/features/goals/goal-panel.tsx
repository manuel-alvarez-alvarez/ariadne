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
import { ChevronRightIcon } from "lucide-react"
import type { ReactNode } from "react"
import { useSearchParams } from "react-router-dom"

import { ApiError, api, type GoalDto, qk, unwrap } from "@/api"
import { CopyableId, CopyableIdMenu } from "@/components/copyable-id"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { StatusBadge } from "@/components/status-badge"
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { goalCopyEntries } from "@/lib/copy-entries"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { usePanelSessionNavigation } from "@/routes/paths"
import { GoalActions } from "./goal-actions"
import { GoalSessions, GoalSessionView } from "./goal-sessions"
import { GoalTasks } from "./goal-tasks"
import { GoalThread } from "./goal-thread"
import { goalQueryOptions } from "./queries"
import { GOAL_STATUS_META } from "./status"

const TABS = ["tasks", "thread", "sessions"] as const
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
  // Tasks first: what a goal *is* is its tasks, and they are otherwise on the
  // board this panel covers.
  const tab = TABS.find((value) => value === search.get("tab")) ?? "tasks"

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
            box="badge"
            label={GOAL_STATUS_META[goal.status].label}
            tone={GOAL_STATUS_META[goal.status].badge}
          />
        </div>
        <CopyableIdMenu
          value={goal.id}
          label="goal id"
          entries={goalCopyEntries(goal.id)}
          className="text-xs text-muted-foreground"
        />
      </SheetHeader>

      <GoalActions goal={goal} />

      {/* Both fold away: what the panel is for is the tabs below, and on a
          laptop an open description and a details card push them off the
          screen. Closed, each still says what it holds. */}
      {goal.description.trim() ? (
        <Fold title="Description" preview={firstLine(goal.description)}>
          <Markdown>{goal.description}</Markdown>
        </Fold>
      ) : null}

      <GoalMetadata goal={goal} />

      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
        <TabsList>
          <TabsTrigger value="tasks">Tasks</TabsTrigger>
          <TabsTrigger value="thread">Thread</TabsTrigger>
          <TabsTrigger value="sessions">Sessions</TabsTrigger>
        </TabsList>
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
 * A card that stays out of the way: closed it is one line — its name and a
 * glimpse of what is inside — open it is its content.
 *
 * A plain `<details>` rather than a primitive of its own: it is what the
 * element is for, and it comes with the keyboard and the screen reader already
 * done. The marker is hidden on both engines Tauri ships with.
 */
function Fold({
  title,
  preview,
  children,
}: {
  title: string
  /** What the fold holds, in a few words, while it is closed. */
  preview?: string
  children: ReactNode
}) {
  return (
    <details className="group rounded-lg border bg-card">
      <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-xs [&::-webkit-details-marker]:hidden">
        <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform group-open:rotate-90" />
        <span className="shrink-0 font-medium">{title}</span>
        {preview ? (
          <span className="truncate text-muted-foreground group-open:hidden">{preview}</span>
        ) : null}
      </summary>
      <div className="border-t px-3 py-2.5">{children}</div>
    </details>
  )
}

/** The first thing the text says, for a fold that is closed over it. */
function firstLine(text: string): string {
  return text.trim().split("\n", 1)[0] ?? ""
}

function GoalMetadata({ goal }: { goal: GoalDto }) {
  const repos = goal.repos.length
  return (
    <Fold
      title="Details"
      preview={`${goal.required_approvals} ${goal.required_approvals === 1 ? "approval" : "approvals"} · ${repos} ${repos === 1 ? "repository" : "repositories"}`}
    >
      <div className="flex flex-col gap-3 text-sm">
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
                <CopyableId value={repo.path} label="repository path" className="text-xs" />
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
      </div>
    </Fold>
  )
}

function Detail({ label, children }: { label: string; children: ReactNode }) {
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
