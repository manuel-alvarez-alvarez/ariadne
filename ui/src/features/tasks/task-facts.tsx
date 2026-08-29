/**
 * What a task *is*, as the card under its header: the branch and worktree it
 * works in, who is on it, what it waits for, and where it landed.
 *
 * Everything here is one click from a terminal — a worktree path, a branch, a
 * merge commit — so each of them is copyable rather than a value to retype, and
 * the ones a person recognises by their tail are truncated in the middle.
 */

import { useQueries } from "@tanstack/react-query"
import { GitBranchIcon, GitCommitHorizontalIcon, GitPullRequestIcon } from "lucide-react"
import type { ReactNode } from "react"
import { Link } from "react-router-dom"

import type { TaskDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { Fact, FactList } from "@/components/fact-list"
import { TokenFigure, taskUsageRows } from "@/components/token-figure"
import { ProfileSummary } from "@/features/profiles/profile-summary"
import { cn, shortSha } from "@/lib/format"
import { useTaskPanelTo } from "@/routes/paths"

import { taskQueryOptions } from "./queries"
import { primaryStatus, TASK_STATUS_META } from "./status"

export function TaskFacts({ task }: { task: TaskDto }) {
  return (
    // Two columns rather than the three a goal's facts take: a branch and a
    // worktree path are the long values in the app, and a third column only
    // cuts them shorter.
    <FactList columns={2}>
      <Fact label="Branch">
        <span className="flex items-center gap-1.5">
          <GitBranchIcon className="size-3.5 shrink-0 text-muted-foreground" />
          <CopyableId value={task.branch} label="branch" truncate="middle" className="text-xs" />
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
        {/* The task's pin, not the profile's live model: editing the profile
            does not move work that is already assigned. */}
        <ProfileSummary
          profileId={task.engineer_profile_id}
          model={task.model}
          className="text-xs"
        />
      </Fact>
      <Fact label="Reviewers">
        {task.reviewers.length > 0 ? (
          // One line each: a reviewer is now a name and what it runs on, which
          // side by side would be a run-on the eye cannot split. Each slot
          // carries its own pin, so two reviewers on the same profile can
          // still read differently.
          <span className="flex flex-col gap-0.5 text-xs">
            {task.reviewers.map((reviewer) => (
              <ProfileSummary
                key={reviewer.profile_id}
                profileId={reviewer.profile_id}
                model={reviewer.model}
              />
            ))}
          </span>
        ) : (
          <Muted>none assigned</Muted>
        )}
      </Fact>
      <Fact label="Tokens">
        {/* Every agent that has run this task, the reviewers included, with
            the hint breaking the same total down by who spent it. */}
        <TokenFigure
          usage={task.usage.total}
          rows={taskUsageRows(task.usage)}
          className="text-xs"
        />
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
      {/* Only a task its engineer published has one, and only then is the
          forge where the rest of its story is — a row saying "no pull request"
          on every locally landed task would say nothing at all. */}
      {task.pr_url ? (
        <Fact label="Pull request">
          <span className="flex min-w-0 items-center gap-1.5">
            <GitPullRequestIcon className="size-3.5 shrink-0 text-muted-foreground" />
            <a
              href={task.pr_url}
              target="_blank"
              rel="noreferrer"
              className="min-w-0 truncate text-xs underline-offset-3 hover:underline"
              title={task.pr_url}
            >
              {task.pr_url}
            </a>
          </span>
        </Fact>
      ) : null}
    </FactList>
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
