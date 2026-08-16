/**
 * One goal: what it is, what it is allowed to do, its board of tasks, and the
 * planner thread.
 *
 * `ariadne goal inspect` plus `ariadne goal messages` and `attach`. The board
 * between them is `src/features/tasks`' `TaskBoard`, mounted here because the
 * goal screen owns the page. Everything reads the query cache, which the SSE
 * dispatcher patches, so a status change made from the CLI or by the daemon
 * lands here on its own.
 */

import { useQuery } from "@tanstack/react-query"
import { AlertCircleIcon, ChevronLeftIcon } from "lucide-react"
import { Link, useParams } from "react-router-dom"

import { ApiError, api, type GoalDto, qk, unwrap } from "@/api"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Skeleton } from "@/components/ui/skeleton"
import { TaskBoard } from "@/features/tasks"
import { paths } from "@/routes/paths"
import { formatAbsolute, formatRelative } from "./format"
import { GoalActions } from "./goal-actions"
import { GoalStatusBadge } from "./goal-status-badge"
import { GoalThread } from "./goal-thread"
import { Markdown } from "./markdown"
import { goalQueryOptions } from "./queries"

export function GoalDetailPage() {
  const { goalId = "" } = useParams<{ goalId: string }>()
  const goal = useQuery({ ...goalQueryOptions(goalId), enabled: goalId !== "" })
  const error = ApiError.is(goal.error) ? goal.error : null

  return (
    <div className="flex flex-col gap-4">
      {/* `nativeButton={false}`: this one renders as the router's <a>, not a <button>. */}
      <Button
        variant="ghost"
        size="sm"
        nativeButton={false}
        className="w-fit"
        render={<Link to={paths.goals()} />}
      >
        <ChevronLeftIcon />
        Goals
      </Button>

      {error ? (
        <Alert variant="destructive">
          <AlertCircleIcon />
          <AlertTitle>
            {error.status === 404 ? "No such goal" : "Could not load the goal"}
          </AlertTitle>
          <AlertDescription>{error.message}</AlertDescription>
          {error.status === 404 ? null : (
            <AlertAction>
              <Button variant="outline" size="sm" onClick={() => void goal.refetch()}>
                Retry
              </Button>
            </AlertAction>
          )}
        </Alert>
      ) : null}

      {goal.isPending ? <Skeleton className="h-32 w-full" /> : null}

      {goal.data ? <GoalView goal={goal.data} /> : null}
    </div>
  )
}

function GoalView({ goal }: { goal: GoalDto }) {
  return (
    <>
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="font-heading text-lg font-semibold">{goal.title}</h1>
            <GoalStatusBadge status={goal.status} />
          </div>
          <p className="font-mono text-xs text-muted-foreground">{goal.id}</p>
        </div>
        <GoalActions goal={goal} />
      </div>

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

      {/* The board is eight columns wide and scrolls; it gets the full width. */}
      <TaskBoard goalId={goal.id} />

      <div className="grid min-h-0 gap-4 lg:grid-cols-3">
        <GoalThread goalId={goal.id} className="lg:col-span-2" />
        <GoalMetadata goal={goal} />
      </div>
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
        <Detail label="Status">
          <GoalStatusBadge status={goal.status} />
        </Detail>
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
                <span className="block font-mono text-xs break-all">{repo.path}</span>
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
      {profile.data?.name ?? <span className="font-mono text-xs">{profileId}</span>}
    </span>
  )
}
