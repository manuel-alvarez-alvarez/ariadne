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

import type { TaskAgentDto, TaskDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { Fact, FactList } from "@/components/fact-list"
import { StatusBadge } from "@/components/status-badge"
import { TokenFigure, taskUsageRows } from "@/components/token-figure"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { AgentSummary } from "@/features/models/agent-summary"
import { SkillName } from "@/features/skills/skill-name"
import { cn, LANDING_LABELS, shortSha } from "@/lib/format"
import { useTaskPanelTo } from "@/routes/paths"
import { taskAuthor, taskAuthors, taskReviewers } from "./agents"

import { taskQueryOptions } from "./queries"
import { primaryStatus, TASK_STATUS_META } from "./status"

export function TaskFacts({ task }: { task: TaskDto }) {
  const author = taskAuthor(task)
  const authors = taskAuthors(task)
  const reviewers = taskReviewers(task)

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
      <Fact label={authors.length > 1 ? "Authors" : "Author"}>
        {authors.length === 0 ? (
          <Muted>none staffed</Muted>
        ) : author && authors.length === 1 ? (
          <AgentSummary
            skills={author.skills}
            model={author.model}
            effort={author.effort}
            className="text-xs"
          />
        ) : (
          // Several authors write the task side by side, each on its own
          // branch: one line apiece, with the branch it owns and its own pick
          // status — the same facts, and the same order, as `ariadne task
          // inspect`'s `author` line.
          <span className="flex flex-col gap-1 text-xs">
            {authors.map((author) => (
              <AuthorRow key={author.id} task={task} author={author} />
            ))}
          </span>
        )}
      </Fact>
      <Fact label="Reviewers">
        {reviewers.length > 0 ? (
          // One line each: a reviewer is its skills and what it runs on, which
          // side by side would be a run-on the eye cannot split. Each agent
          // carries its own pin, so two reviewers on the same skills can still
          // read differently.
          <span className="flex flex-col gap-0.5 text-xs">
            {reviewers.map((reviewer) => (
              <AgentSummary
                key={reviewer.id}
                skills={reviewer.skills}
                model={reviewer.model}
                effort={reviewer.effort}
              />
            ))}
          </span>
        ) : (
          <Muted>none staffed</Muted>
        )}
      </Fact>
      {/* Only a task staffed with several authors has a pick to show: a
          one-author task is approved the moment its author asks, and prints
          exactly what it always did (parity with `task inspect`'s own
          `picks` line, which is likewise left out below one author). */}
      {authors.length > 1 ? (
        <Fact label="Picks">
          <Picks task={task} />
        </Fact>
      ) : null}
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
      <Fact label="Ends with">
        {/* Agreed with the user when the task was written, so it says what
            will happen rather than what happened: the merge commit and the
            request below are the record of what did. */}
        <span className="text-xs">{LANDING_LABELS[task.landing]}</span>
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
          <Muted>{task.landing === "none" ? "nothing to merge" : "not merged"}</Muted>
        )}
      </Fact>
      {/* Only a task its author published has one, and only then is the
          forge where the rest of its story is — a row saying "no pull request"
          on every locally landed task would say nothing at all. */}
      {task.pr_url ? (
        <Fact label="Pull request">
          <span className="flex min-w-0 items-center gap-1.5">
            <GitPullRequestIcon className="size-3.5 shrink-0 text-muted-foreground" />
            {/* Truncated to the card's width, so the whole URL is the hint —
                a real one, reachable from the link's own focus. */}
            <Tooltip>
              <TooltipTrigger
                render={
                  <a
                    href={task.pr_url}
                    target="_blank"
                    rel="noreferrer"
                    className="min-w-0 truncate text-xs underline-offset-3 hover:underline"
                  />
                }
              >
                {task.pr_url}
              </TooltipTrigger>
              <TooltipContent>{task.pr_url}</TooltipContent>
            </Tooltip>
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
 * One author of a several-author task: what it knows, what it runs on, its
 * own branch, and its status in the pick — the votes it has, or the "Picked"
 * mark once it is the one that won. Every author gets one of the two: the
 * task's own status (in the header above) says where the task as a whole
 * stands, not which of several candidates is ahead.
 */
function AuthorRow({ task, author }: { task: TaskDto; author: TaskAgentDto }) {
  const votes = task.picks.filter((pick) => pick.author_agent_id === author.id).length
  return (
    <span className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
      <AgentSummary skills={author.skills} model={author.model} effort={author.effort} />
      <span className="flex min-w-0 items-center gap-1 text-muted-foreground">
        <GitBranchIcon className="size-3 shrink-0" />
        {author.branch ? (
          <CopyableId value={author.branch} label="branch" truncate="middle" />
        ) : (
          <span className="italic">no branch</span>
        )}
      </span>
      {task.picked_agent_id === author.id ? (
        <StatusBadge label="Picked" tone={TASK_STATUS_META.approved.badge} size="sm" />
      ) : (
        <span className="text-muted-foreground">
          {/* "No picks yet" is true of the task, not this author: once the
              first pick lands, an author with none of its own has "0 picks",
              not the unsettled wording that reads as nobody having voted at
              all. */}
          {task.picks.length === 0 ? "no picks yet" : `${votes} ${votes === 1 ? "pick" : "picks"}`}
        </span>
      )}
    </span>
  )
}

/**
 * The reviewer picks recorded so far, oldest first: who picked whom, named the
 * way an agent is named everywhere else — the skills it carries. Matches
 * `ariadne task inspect`'s `picks` line.
 */
function Picks({ task }: { task: TaskDto }) {
  if (task.picks.length === 0) return <Muted>no picks yet</Muted>
  return (
    <ul className="flex flex-col gap-1 text-xs">
      {task.picks.map((pick) => (
        <li key={pick.reviewer_agent_id} className="min-w-0">
          <StaffedLabel task={task} agentId={pick.reviewer_agent_id} /> picked{" "}
          <StaffedLabel task={task} agentId={pick.author_agent_id} />
        </li>
      ))}
    </ul>
  )
}

/**
 * An agent staffed on the task, named by its skills; a pick's agent id that no
 * longer staffs the task — which the daemon does not produce today, but a
 * screen still has to render rather than throw on — falls back to the id.
 */
function StaffedLabel({ task, agentId }: { task: TaskDto; agentId: string }) {
  const agent = task.agents.find((candidate) => candidate.id === agentId)
  if (!agent) return <span className="font-mono">{agentId}</span>
  if (agent.skills.length === 0) {
    return <span className="text-muted-foreground italic">no skills</span>
  }
  return (
    <>
      {agent.skills.map((skill, at) => (
        <span key={skill}>
          {at > 0 ? ", " : null}
          <SkillName name={skill} />
        </span>
      ))}
    </>
  )
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
    // The id behind the name, for the terminal the reader is about to type it
    // into — as a hint the keyboard reaches, like every other one in the app.
    <Tooltip>
      <TooltipTrigger
        render={
          <Link to={to} replace className="truncate text-xs underline-offset-3 hover:underline" />
        }
      >
        {title ?? id}
      </TooltipTrigger>
      <TooltipContent className="font-mono">{id}</TooltipContent>
    </Tooltip>
  )
}
