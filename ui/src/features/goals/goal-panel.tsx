/**
 * One goal in a side panel over the goals board: what it is, what it is
 * allowed to do, and the planner thread. `ariadne goal inspect` plus
 * `goal messages` — its tasks stay on the board behind the panel.
 *
 * Everything reads the query cache, which the SSE dispatcher patches, so a
 * status change made from the CLI or by the daemon lands here on its own.
 */

import { useQuery } from "@tanstack/react-query"
import { AlertCircleIcon } from "lucide-react"

import { ApiError, api, type GoalDto, qk, unwrap } from "@/api"
import { Alert, AlertAction, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Sheet, SheetContent, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Skeleton } from "@/components/ui/skeleton"
import { formatAbsolute, formatRelative } from "./format"
import { GoalActions } from "./goal-actions"
import { GoalStatusBadge } from "./goal-status-badge"
import { GoalThread } from "./goal-thread"
import { Markdown } from "./markdown"
import { goalQueryOptions } from "./queries"

export function GoalPanel({ goalId, onClose }: { goalId: string; onClose: () => void }) {
  const goal = useQuery(goalQueryOptions(goalId))
  const error = ApiError.is(goal.error) ? goal.error : null

  return (
    <Sheet open onOpenChange={(open) => open || onClose()}>
      <SheetContent aria-describedby={undefined}>
        {error ? (
          <>
            <SheetTitle className="sr-only">Goal {goalId}</SheetTitle>
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
          </>
        ) : null}

        {goal.isPending ? (
          <>
            <SheetTitle className="sr-only">Loading goal</SheetTitle>
            <Skeleton className="h-7 w-2/3" />
            <Skeleton className="h-40 w-full" />
          </>
        ) : null}

        {goal.data ? <GoalView goal={goal.data} /> : null}
      </SheetContent>
    </Sheet>
  )
}

function GoalView({ goal }: { goal: GoalDto }) {
  return (
    <>
      <SheetHeader>
        <div className="flex flex-wrap items-center gap-2">
          <SheetTitle>{goal.title}</SheetTitle>
          <GoalStatusBadge status={goal.status} />
        </div>
        <p className="font-mono text-xs text-muted-foreground">{goal.id}</p>
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

      <GoalThread goalId={goal.id} />
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
