/**
 * One attention item as a block: the one component the popover, the drawer and
 * the goals board draw an item with.
 */

import { useIsMutating, useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link, useLocation, useSearchParams } from "react-router-dom"

import { api, type GoalDto, type Host, qk, unwrap } from "@/api"
import { HostLabel } from "@/components/host-label"
import { StatusBadge } from "@/components/status-badge"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { When } from "@/components/when"
import {
  answerQuestion,
  choiceLabel,
  formFields,
  listQuestions,
  questionSteps,
} from "@/features/sessions/console/form-questions"
import { SessionAttentionBadge } from "@/features/sessions/session-display"
import {
  applySnapshot,
  emptyTrajectory,
  pendingPermissions,
} from "@/features/sessions/trajectory/store"
import { StalledBadge, TASK_STATUS_META } from "@/features/tasks"
import { useHasNamedHosts } from "@/hooks/use-host-filter"
import { describeError, shortId } from "@/lib/format"

import {
  type AttentionItem,
  attentionAffected,
  attentionAffectedLinks,
  attentionDetail,
  attentionListQueryOptions,
  attentionSubject,
  attentionTarget,
  useFollowAttention,
} from "./attention"

/** Names where {@link attentionTarget} will go, so the label never misstates it. */
function actionLabel(item: AttentionItem): string {
  if (item.recovery) {
    switch (item.recovery.target.kind) {
      case "console":
        return "Open console"
      case "goal":
        return "Open goal"
      case "task":
        return "Open task"
      case "pull_request":
        return "Open pull request"
      case "settings":
        return item.recovery.target.section === "agents" ? "Open agents" : "Open repositories"
    }
  }
  if (
    item.session &&
    (item.sessionReason === "waiting_permission" || item.sessionReason === "waiting_input")
  )
    return "Open console"
  if (!item.taskReason && item.session) return "Open session"
  return "Open task"
}

/**
 * One thing that wants a person: what it is, why, and the way to answer it.
 *
 * The badges are the reasons, task first — a task's status is what the row
 * sits under, and the session's reason is the live thing on top of it. Where
 * the row goes is the reason's to say (see {@link attentionTarget}), so a
 * question opens the thread it was asked in and a prompt opens the console it
 * is waiting in.
 */
export function AttentionBlock({
  item,
  onSelect,
  expanded = false,
}: {
  item: AttentionItem
  onSelect?: () => void
  /** The drawer's row: the whole text, and the action named. */
  expanded?: boolean
}) {
  const queryClient = useQueryClient()
  // The list spans every reachable host whatever the pick.
  const showHost = useHasNamedHosts()
  const dismiss = useMutation({
    mutationFn: () =>
      unwrap(
        api(item.host).POST("/v1/attention/{id}/dismiss", { params: { path: { id: item.id } } }),
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: qk.attention.lists(item.host) }),
  })
  const [search] = useSearchParams()
  // Where a row lands depends on the screen as well as its params: the strip is
  // the board's, but the same targets are used from the alerts, which are
  // everywhere (see {@link attentionTarget}).
  const { pathname } = useLocation()
  const status = item.taskReason && item.task ? TASK_STATUS_META[item.task.status] : null
  const target = attentionTarget(item, search, pathname)
  const follow = useFollowAttention()
  // A grouped quota item's own target opens only one of its sessions;
  // every one of them gets its own small link below, so none of them is
  // unreachable just because another one happened to be first.
  const affectedLinks = attentionAffectedLinks(item, search, pathname)
  const className =
    "flex flex-wrap items-center gap-x-2 gap-y-1 px-3 py-2 text-sm transition-colors hover:bg-muted/50"
  const content = (
    <>
      {status ? (
        <StatusBadge box="badge" label={status.label} tone={status.badge} hint={status.hint} />
      ) : null}
      {/* The status pill already names a failed task; a stall is a flag on
          top of whatever status the task is sitting in. */}
      {item.taskReason === "stalled" ? <StalledBadge /> : null}
      {item.sessionReason ? <SessionAttentionBadge attention={item.sessionReason} /> : null}
      {expanded ? (
        <span className="min-w-0 flex-1">
          <span className="block font-medium break-words">{attentionSubject(item)}</span>
          <span className="block text-xs break-words text-muted-foreground">
            {attentionDetail(item)}
          </span>
          {attentionAffected(item) ? (
            <span className="block text-xs break-words text-muted-foreground">
              Affects: {attentionAffected(item)}
            </span>
          ) : null}
        </span>
      ) : (
        <Subject
          subject={attentionSubject(item)}
          detail={attentionDetail(item)}
          affected={attentionAffected(item)}
        />
      )}
      {/* Its own line while the strip is narrow, where the goal, the stamp
          and the id together took half the row and left the subject twenty
          characters. */}
      <div className="flex w-full items-center gap-2 @2xl:w-auto">
        {/* A pull request's session belongs to no goal to name. */}
        {item.goalId ? <GoalRef goalId={item.goalId} goal={item.goal} /> : null}
        {showHost ? <HostLabel host={item.host} /> : null}
        {/* One label for every row: a task's row moves when the task is
            updated, a session's when it started asking — "last moved" is the
            one thing true of both, and the list is ordered by it. */}
        <When at={item.at} label="last moved" className="text-xs text-muted-foreground" />
        <RowId id={item.id} />
        {expanded ? (
          <span className="text-xs font-medium underline">{actionLabel(item)}</span>
        ) : null}
      </div>
    </>
  )

  return (
    <li>
      {item.recovery ? (
        <button
          type="button"
          disabled={dismiss.isPending}
          onClick={() => dismiss.mutate()}
          className="float-right px-3 py-2 text-xs underline"
        >
          Dismiss
        </button>
      ) : null}
      {dismiss.error ? <p role="alert">{describeError(dismiss.error)}</p> : null}
      <Link
        to={target}
        // Set exactly when the target is a task panel replacing an open
        // goal's, rather than stacking on it (`target.replace`).
        replace={target.replace}
        onClick={() => {
          follow(target)
          onSelect?.()
        }}
        className={className}
      >
        {content}
      </Link>
      {item.recovery?.producer === "agent_request" &&
      item.recovery.target.kind === "console" &&
      item.recovery.id.startsWith("question:") ? (
        <FormAnswers
          host={item.host}
          sessionId={item.recovery.target.session_id}
          questionId={item.recovery.id.slice("question:".length)}
        />
      ) : null}
      {item.recovery?.producer === "agent_request" &&
      item.recovery.target.kind === "console" &&
      item.recovery.id.startsWith("agent_request:waiting_permission:") ? (
        <PermissionAnswers host={item.host} sessionId={item.recovery.target.session_id} />
      ) : null}
      {affectedLinks ? (
        <div className="flex flex-wrap gap-2 px-3 pb-2 pl-9 text-xs text-muted-foreground">
          <span>Open:</span>
          {affectedLinks.map((link) => (
            <Link
              key={link.id}
              to={link.to}
              className="underline underline-offset-2 hover:text-foreground"
            >
              {link.label}
            </Link>
          ))}
        </div>
      ) : null}
    </li>
  )
}

/**
 * What the row is about, over what it is: the title on top in the row's own
 * size, the explanation under it in the muted one.
 *
 * Two lines rather than one, because the two used to compete for the same
 * truncating span — and it was the title, the only part that identifies the
 * row, that lost first. They now truncate independently, so a narrow strip
 * shortens both instead of dropping one.
 *
 * Both still truncate, so what a row says in full is only ever in the hint —
 * one `Tooltip` over the pair rather than a `title=` on each, which is what
 * puts it in reach of a keyboard.
 */
function Subject({
  subject,
  detail,
  affected,
}: {
  subject: string
  detail: string
  /** A recovery row's own `affected` list, joined — null for every other row. */
  affected: string | null
}) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="min-w-0 flex-1" />}>
        <span className="block truncate font-medium">{subject}</span>
        <span className="block truncate text-xs text-muted-foreground">{detail}</span>
        {affected ? (
          <span className="block truncate text-xs text-muted-foreground">Affects: {affected}</span>
        ) : null}
      </TooltipTrigger>
      <TooltipContent className="flex-col items-start gap-0.5">
        <span className="font-medium">{subject}</span>
        <span className="text-background/70">{detail}</span>
        {affected ? <span className="text-background/70">Affects: {affected}</span> : null}
      </TooltipContent>
    </Tooltip>
  )
}

/**
 * Which goal this row belongs to — the one thing the old grouped screen said
 * that a flat list otherwise loses.
 *
 * Plain text rather than a link to the goal: the row is already a link, and
 * the panel it opens carries the goal anyway.
 */
function GoalRef({ goalId, goal }: { goalId: string; goal: GoalDto | undefined }) {
  const title = goal?.title ?? `Goal ${shortId(goalId)}`
  return (
    <Tooltip>
      <TooltipTrigger
        render={<span className="max-w-40 shrink-0 truncate text-xs text-muted-foreground" />}
      >
        {title}
      </TooltipTrigger>
      <TooltipContent>{title}</TooltipContent>
    </Tooltip>
  )
}

/**
 * A question an agent asked with its own tool, answered in place when its form
 * is one single-select question: one button per choice, posted to the
 * question's own route. Any other form is answered in the console, which the
 * row's own link opens.
 */
function FormAnswers({
  host,
  sessionId,
  questionId,
}: {
  host: Host
  sessionId: string
  questionId: string
}) {
  const queryClient = useQueryClient()
  const questions = useQuery({
    queryKey: qk.sessions.questions(host, sessionId),
    queryFn: () => listQuestions(host, sessionId),
  })
  const answer = useMutation({
    mutationFn: (content: Record<string, unknown>) =>
      answerQuestion(host, sessionId, questionId, { action: "accept", content }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: qk.sessions.questions(host, sessionId) })
      void queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) })
    },
  })
  const asked = questions.data?.find((question) => question.id === questionId)
  if (!asked) return null
  const [step, ...rest] = questionSteps(formFields(asked.schema))
  const field = step?.field
  if (!field || rest.length > 0 || field.kind.type !== "single") return null
  const settled = answer.isPending || answer.isSuccess
  return (
    <section aria-label="Agent question" className="flex flex-col gap-1 px-3 pb-2 text-sm">
      {asked.message === "" ? null : (
        <p className="whitespace-pre-wrap text-foreground">{asked.message}</p>
      )}
      <fieldset aria-label={field.title} className="flex flex-wrap gap-1">
        {field.kind.choices.map((choice) => (
          <Button
            key={String(choice.value)}
            type="button"
            size="sm"
            variant="outline"
            disabled={settled}
            title={choice.description}
            onClick={() => answer.mutate({ [field.key]: choice.value })}
          >
            {choiceLabel(choice.title)}
            {choiceLabel(choice.title) !== choice.title ? (
              <Badge className="text-sm">Recommended</Badge>
            ) : null}
          </Button>
        ))}
      </fieldset>
      {answer.isSuccess ? <p role="status">Answer sent.</p> : null}
      {answer.error ? (
        <p role="alert" className="text-destructive">
          Could not send the answer: {describeError(answer.error)}
        </p>
      ) : null}
    </section>
  )
}

/**
 * The oldest permission a session waits for, answered in place: one button per
 * option, each posting its id as `text` to the console input of the session.
 * It reads the session's trajectory once for each move of the host's attention
 * list, so a new or an answered permission shows with no poll of its own.
 */
function PermissionAnswers({ host, sessionId }: { host: Host; sessionId: string }) {
  const queryClient = useQueryClient()
  // Observes the list the rows come from; it never fetches it itself.
  const { dataUpdatedAt } = useQuery({ ...attentionListQueryOptions(host), enabled: false })
  const trajectory = useQuery({
    queryKey: qk.sessions.pendingPermission(host, sessionId, dataUpdatedAt),
    queryFn: async () => {
      const snapshot = await unwrap(
        api(host).GET("/v1/sessions/{id}/trajectory", { params: { path: { id: sessionId } } }),
      )
      return pendingPermissions(applySnapshot(emptyTrajectory(), snapshot))[0] ?? null
    },
  })
  // The board, the popover and the drawer can show the same permission, so
  // a post running in any one of them disables the buttons of all.
  const answerKey = qk.sessions.permissionAnswer(host, sessionId)
  const posting = useIsMutating({ mutationKey: answerKey }) > 0
  const answer = useMutation({
    mutationKey: answerKey,
    mutationFn: (optionId: string) =>
      unwrap(
        api(host).POST("/v1/sessions/{id}/console/input", {
          params: { path: { id: sessionId } },
          body: { text: optionId },
        }),
      ),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: qk.attention.lists(host) }),
  })
  const step = trajectory.data
  if (!step) return null
  return (
    <section aria-label="Pending permission" className="flex flex-col gap-1 px-3 pb-2 text-sm">
      <p className="font-medium break-words">
        Permission · {step.tool.title}
        {step.subagent ? (
          <span className="font-normal text-muted-foreground">
            {" "}
            · asked by subagent {step.subagent.title}
          </span>
        ) : null}
      </p>
      {step.tool.subject ? (
        <code className="block rounded-md bg-muted px-2 py-1 font-mono text-xs break-words whitespace-pre-wrap">
          {step.tool.subject}
        </code>
      ) : null}
      {step.question && step.question !== step.tool.title ? (
        <p className="break-words">{step.question}</p>
      ) : null}
      <div className="flex flex-wrap gap-1">
        {step.options.map((option) => (
          <Button
            key={option.id}
            type="button"
            size="sm"
            variant="outline"
            disabled={posting}
            onClick={() => answer.mutate(option.id)}
          >
            {option.name}
          </Button>
        ))}
      </div>
      {answer.error ? (
        <p role="alert" className="text-destructive">
          Could not answer the permission: {describeError(answer.error)}
        </p>
      ) : null}
    </section>
  )
}

function RowId({ id }: { id: string }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<span className="font-mono text-xs text-muted-foreground" />}>
        {shortId(id)}
      </TooltipTrigger>
      <TooltipContent className="font-mono">{id}</TooltipContent>
    </Tooltip>
  )
}
