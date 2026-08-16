/**
 * Reviews per round — the `task reviews` equivalent.
 *
 * A task can go around several times, so the rounds are what structures the
 * tab: newest first, each with the verdicts that closed it.
 */

import { useQuery } from "@tanstack/react-query"
import { CheckCircle2Icon, MessageSquareWarningIcon } from "lucide-react"
import { useMemo } from "react"

import type { ReviewDto, ReviewVerdict } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Markdown } from "@/components/markdown"
import { Skeleton } from "@/components/ui/skeleton"
import { formatAbsolute, formatRelative } from "@/lib/time"
import { cn } from "@/lib/utils"
import { taskReviewsQueryOptions } from "./queries"
import { SessionLink } from "./task-sessions"

const VERDICT_META: Record<
  ReviewVerdict,
  { label: string; badge: string; icon: typeof CheckCircle2Icon }
> = {
  approve: {
    label: "Approved",
    badge: "bg-emerald-500/12 text-emerald-700 dark:bg-emerald-400/15 dark:text-emerald-300",
    icon: CheckCircle2Icon,
  },
  request_changes: {
    label: "Changes requested",
    badge: "bg-orange-500/12 text-orange-700 dark:bg-orange-400/15 dark:text-orange-300",
    icon: MessageSquareWarningIcon,
  },
}

export function TaskReviews({ taskId }: { taskId: string }) {
  const reviews = useQuery(taskReviewsQueryOptions(taskId))
  const rounds = useMemo(() => groupByRound(reviews.data ?? []), [reviews.data])

  if (reviews.isPending) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-24 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    )
  }

  if (reviews.error) {
    return (
      <ErrorState
        title="Could not load the reviews"
        error={reviews.error}
        onRetry={() => void reviews.refetch()}
      />
    )
  }

  if (rounds.length === 0) {
    return <EmptyState title="No review has been submitted yet." />
  }

  return (
    <div className="space-y-5">
      {rounds.map(([round, entries]) => (
        <section key={round} className="space-y-2">
          <h3 className="font-heading text-sm font-medium">
            Round {round}
            <span className="ml-2 text-xs font-normal text-muted-foreground">
              {entries.length} {entries.length === 1 ? "verdict" : "verdicts"}
            </span>
          </h3>
          {entries.map((review) => (
            <ReviewCard key={review.id} review={review} />
          ))}
        </section>
      ))}
    </div>
  )
}

function ReviewCard({ review }: { review: ReviewDto }) {
  const { label, badge, icon: Icon } = VERDICT_META[review.verdict]
  return (
    <article className="rounded-lg border bg-card px-3 py-2">
      <header className="mb-1.5 flex flex-wrap items-center gap-2 text-xs">
        <span
          className={cn("flex items-center gap-1 rounded-full px-1.5 py-0.5 font-medium", badge)}
        >
          <Icon className="size-3" />
          {label}
        </span>
        <CopyableId
          value={review.reviewer_profile_id}
          label="profile id"
          className="text-muted-foreground"
        />
        {review.session_id && <SessionLink sessionId={review.session_id} />}
        <time
          className="ml-auto text-muted-foreground"
          dateTime={review.created_at}
          title={formatAbsolute(review.created_at)}
        >
          {formatRelative(review.created_at)}
        </time>
      </header>
      {review.body ? (
        <Markdown>{review.body}</Markdown>
      ) : (
        <p className="text-sm text-muted-foreground italic">No comment.</p>
      )}
    </article>
  )
}

/** Newest round first; within a round, in the order the verdicts landed. */
function groupByRound(reviews: ReviewDto[]): [number, ReviewDto[]][] {
  const rounds = new Map<number, ReviewDto[]>()
  for (const review of reviews) {
    const entries = rounds.get(review.round)
    if (entries) entries.push(review)
    else rounds.set(review.round, [review])
  }
  return [...rounds.entries()].sort(([a], [b]) => b - a)
}
