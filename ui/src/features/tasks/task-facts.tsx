import { useQueries } from "@tanstack/react-query"
import { GitBranchIcon, GitCommitHorizontalIcon, GitPullRequestIcon } from "lucide-react"
import type { ReactNode } from "react"
import { Link } from "react-router-dom"

import type { TaskDto, WorkflowStepDto } from "@/api"
import { CopyableId } from "@/components/copyable-id"
import { Fact, FactList } from "@/components/fact-list"
import { stepUsageRows, TokenFigure } from "@/components/token-figure"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { AgentSummary } from "@/features/models/agent-summary"
import { cn, shortSha } from "@/lib/format"
import { useTaskPanelTo } from "@/routes/paths"
import { taskQueryOptions } from "./queries"
import { TASK_STATUS_META } from "./status"
import { stepTitle } from "./steps"
import { SessionLink } from "./task-sessions"

export function TaskFacts({ task, steps = [] }: { task: TaskDto; steps?: WorkflowStepDto[] }) {
  return (
    <FactList dense>
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
      <Fact label="Agents">
        <StepAgents task={task} steps={steps} />
      </Fact>
      <Fact label="Tokens">
        <TokenFigure
          usage={task.usage.total}
          rows={stepUsageRows(task.usage, (step) => stepTitle(steps, step))}
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
      {task.pr_url ? (
        <Fact label="Pull request">
          <a
            href={task.pr_url}
            target="_blank"
            rel="noreferrer"
            className="flex items-center gap-1.5 text-xs underline-offset-3 hover:underline"
          >
            <GitPullRequestIcon className="size-3.5" />
            {task.pr_url}
          </a>
        </Fact>
      ) : null}
    </FactList>
  )
}

function StepAgents({ task, steps }: { task: TaskDto; steps: WorkflowStepDto[] }) {
  return (
    <ul className="flex flex-col gap-2 text-xs">
      {steps.map((step) => {
        const agent = task.agents.find((candidate) => candidate.step === step.id)
        return (
          <li key={step.id} className="flex flex-col gap-0.5">
            <span className="flex items-center gap-2">
              <span className="font-medium">{step.title}</span>
              {agent?.session_id ? <SessionLink sessionId={agent.session_id} /> : null}
            </span>
            {agent ? (
              <AgentSummary skills={agent.skills} model={agent.model} effort={agent.effort} />
            ) : (
              <Muted>none staffed</Muted>
            )}
          </li>
        )
      })}
    </ul>
  )
}

function Dependencies({ ids }: { ids: string[] }) {
  const results = useQueries({ queries: ids.map((id) => taskQueryOptions(id)) })
  if (ids.length === 0) return <Muted>nothing</Muted>
  return (
    <ul className="flex flex-col gap-1">
      {ids.map((id, index) => (
        <Dependency key={id} id={id} task={results[index]?.data} />
      ))}
    </ul>
  )
}

function Dependency({ id, task }: { id: string; task?: TaskDto }) {
  const to = useTaskPanelTo(id)
  const status = task ? TASK_STATUS_META[task.status] : undefined
  return (
    <li className="flex items-center gap-1.5">
      {status ? <span className={cn("size-1.5 rounded-full", status.dot)} /> : null}
      <Tooltip>
        <TooltipTrigger
          render={
            <Link to={to} replace className="truncate text-xs underline-offset-3 hover:underline" />
          }
        >
          {task?.title ?? id}
        </TooltipTrigger>
        <TooltipContent>{id}</TooltipContent>
      </Tooltip>
    </li>
  )
}

function Muted({ children }: { children: ReactNode }) {
  return <span className="text-xs text-muted-foreground">{children}</span>
}
