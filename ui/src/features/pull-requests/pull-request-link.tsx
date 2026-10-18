/**
 * The pull request a session watches (026), as its title and a link to the
 * forge: what the sessions table and the session panel show where another
 * session shows its goal and its task.
 *
 * The session carries only the request's id, so the URL is read off the
 * request's own detail key — the one its panel reads and the dispatcher
 * patches. Until that answers, the session's own title stands in, unlinked.
 * The link opens in the browser, the way every forge link in the app does.
 */

import { useQuery } from "@tanstack/react-query"
import { GitPullRequestIcon } from "lucide-react"

import { cn, shortId } from "@/lib/format"

import { pullRequestQueryOptions } from "./queries"

export function PullRequestLink({
  id,
  title,
  className,
}: {
  /** The session's `pull_request_id`. */
  id: string
  /** The session's own title, which is the request's title at launch. */
  title?: string | null
  className?: string
}) {
  const pull = useQuery(pullRequestQueryOptions(id))
  const name = pull.data?.title ?? title ?? `Pull request ${shortId(id)}`
  return (
    <span className={cn("flex min-w-0 items-center gap-1", className)}>
      <GitPullRequestIcon aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
      {pull.data ? (
        <a
          href={pull.data.url}
          target="_blank"
          rel="noreferrer"
          title={`Pull request: ${name}`}
          className="min-w-0 truncate underline-offset-2 hover:underline"
        >
          {name}
        </a>
      ) : (
        <span className="min-w-0 truncate">{name}</span>
      )}
    </span>
  )
}
