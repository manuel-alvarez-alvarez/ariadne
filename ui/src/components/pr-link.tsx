import { GitPullRequestIcon } from "lucide-react"
import type { MouseEventHandler } from "react"

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

/** The request number a forge URL names (`/pull/12`, `/pulls/12`, `/merge_requests/12`). */
export function pullRequestNumber(url: string): number | null {
  const match = /\/(?:pulls?|merge_requests)\/(\d+)(?:[/?#]|$)/.exec(url)
  return match ? Number(match[1]) : null
}

function repositoryOf(url: string): string | null {
  try {
    const path = new URL(url).pathname.split("/").filter(Boolean)
    const at = path.findIndex(
      (part) => part === "pull" || part === "pulls" || part === "merge_requests",
    )
    return at > 1 ? path.slice(0, at).join("/").replace(/\/-$/, "") : null
  } catch {
    return null
  }
}

/** What the compact link shows for a request number: `#12`, or `PR` with none. */
export function pullRequestText(number: number | null): string {
  return number != null ? `#${number}` : "PR"
}

/**
 * A pull request destination as an icon and `#number`: the compact link every
 * PR reference shares. The href is the real URL; hovering shows the repository
 * and that URL. With no identifiable number it shows the icon and "PR".
 */
export function PrLink({
  url,
  number,
  repository,
  context,
  className,
  tabIndex,
  onClick,
}: {
  url: string
  number?: number | null
  repository?: string | null
  /** What the link belongs to, for its accessible name. */
  context?: string
  className?: string
  tabIndex?: number
  onClick?: MouseEventHandler<HTMLAnchorElement>
}) {
  const n = number ?? pullRequestNumber(url)
  const repo = repository ?? repositoryOf(url)
  const text = pullRequestText(n)
  const name = `Pull request ${n != null ? `#${n}` : ""}${repo ? ` in ${repo}` : ""}${context ? `: ${context}` : ""}`
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <a
            href={url}
            target="_blank"
            rel="noreferrer"
            aria-label={name.replace(/\s+/g, " ").trim()}
            data-pr-link
            tabIndex={tabIndex}
            onClick={onClick}
            className={cn(
              "inline-flex max-w-full min-w-0 items-center gap-1 text-xs underline-offset-3 hover:underline",
              className,
            )}
          />
        }
      >
        <GitPullRequestIcon aria-hidden className="size-3.5 shrink-0" />
        <span className="min-w-0 truncate">{text}</span>
      </TooltipTrigger>
      <TooltipContent className="max-w-sm break-all">
        {repo ? <div>{repo}</div> : null}
        <div>{url}</div>
      </TooltipContent>
    </Tooltip>
  )
}
