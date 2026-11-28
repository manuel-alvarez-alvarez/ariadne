/**
 * One step of a turn: its row, and its detail, which a click, Enter or Space
 * on the row opens under it to a set height. The same action closes it.
 *
 * The detail draws by the kind of the step. A tool call reads its typed
 * detail from `GET /v1/sessions/{id}/trajectory/items/{item_id}` (040), again
 * at each new `detail_rev`, so an open step of a running call shows new
 * progress:
 *
 * - a command shows the command and its output;
 * - an edit shows each file's diff in the task diff viewer, with line
 *   numbers and language colours;
 * - a read shows the file with its language and its line numbers;
 * - a search and a fetch show the query or the URL, and the result;
 * - any other call shows its content blocks.
 *
 * A subagent step opens to its own steps, from the steps route of its turn,
 * each a row that opens in place. Its row has a cancel control where the
 * agent permits it.
 *
 * The content comes first and the controls second. Each code section has one
 * frame and its own Copy. `Open` shows the step whole in the full view, and
 * `Raw` reads the call again with `raw=true` and shows the raw input and the
 * raw output: no raw data shows without it.
 */

import { keepPreviousData, useQuery } from "@tanstack/react-query"
import { BracesIcon, CopyIcon, Maximize2Icon } from "lucide-react"
import { useEffect, useMemo, useState } from "react"
import { toast } from "sonner"

import { api, type Host, qk, type StepDto, unwrap } from "@/api"
import { CodeBlock } from "@/components/code-block"
import { IconButton } from "@/components/icon-button"
import { Markdown } from "@/components/markdown"
import { Button } from "@/components/ui/button"
import { copyText } from "@/lib/clipboard"
import { describeError } from "@/lib/format"
import { languageLabel } from "@/lib/highlight"

import { DiffEditor } from "../../tasks/diff-editor"
import { parseUnifiedDiff } from "../../tasks/diff-parse"
import { MediaBlocks } from "./content-blocks"
import { useChildSteps, useFullView } from "./full-view-context"
import type { PermissionStep } from "./notices"
import { StepRow } from "./step-row"
import { childSteps, contentText } from "./store"

type ToolStep = Extract<StepDto, { type: "tool_call" }>
type SubagentStep = Extract<StepDto, { type: "subagent" }>
type ToolDetail = NonNullable<Awaited<ReturnType<typeof readDetail>>>["detail"]
type DiffFileDetail = Extract<ToolDetail, { type: "diff" }>["files"][number]

/** How many steps of a subagent its detail reads. */
const SUBAGENT_STEPS = 200

export function StepDetail({
  host,
  sessionId,
  step,
  now,
  permission,
}: {
  host: Host
  sessionId: string
  step: StepDto
  /** Now, as an ISO instant, for the clock of what still runs. */
  now: string
  permission?: PermissionStep
}) {
  const [open, setOpen] = useState(false)
  return (
    <div>
      <div className="flex min-w-0 items-center gap-1">
        <StepRow
          step={step}
          now={now}
          permission={permission}
          open={open}
          onToggle={() => setOpen((was) => !was)}
        />
        {step.type === "subagent" && step.can_cancel ? (
          <CancelSubagent host={host} sessionId={sessionId} step={step} />
        ) : null}
      </div>
      {open ? (
        <div
          data-step-detail={step.id}
          className="mt-0.5 mb-1 ml-4 max-h-80 overflow-y-auto rounded-md px-1 py-1"
        >
          <StepBody host={host} sessionId={sessionId} step={step} now={now} />
        </div>
      ) : null}
    </div>
  )
}

/** The content of a step by its kind, then its controls. `full` is the full view's. */
export function StepBody({
  host,
  sessionId,
  step,
  now,
  full = false,
}: {
  host: Host
  sessionId: string
  step: StepDto
  now: string
  full?: boolean
}) {
  if (step.type === "tool_call")
    return <ToolBody host={host} sessionId={sessionId} step={step} full={full} />
  if (step.type === "subagent")
    return <SubagentBody host={host} sessionId={sessionId} step={step} now={now} full={full} />
  return (
    <div className="space-y-1.5">
      <PlainBody host={host} step={step} />
      <Controls step={step} full={full} />
    </div>
  )
}

/** `Open` and, for a tool call, `Raw`: under the content. */
function Controls({
  step,
  full,
  raw,
  onRaw,
}: {
  step: StepDto
  full: boolean
  raw?: boolean
  onRaw?: () => void
}) {
  const view = useFullView()
  const open = !full && view !== null
  if (!open && !onRaw) return null
  return (
    <div className="flex items-center gap-1">
      {open ? (
        <IconButton
          size="icon-xs"
          icon={<Maximize2Icon />}
          label="Open"
          onClick={(event) => view.show(step, event.currentTarget)}
        />
      ) : null}
      {onRaw ? (
        <IconButton
          size="icon-xs"
          variant={raw ? "secondary" : "outline"}
          icon={<BracesIcon />}
          label="Raw"
          aria-pressed={raw}
          onClick={onRaw}
        />
      ) : null}
    </div>
  )
}

function readDetail(host: Host, sessionId: string, itemId: string, raw: boolean) {
  return unwrap(
    api(host).GET("/v1/sessions/{id}/trajectory/items/{item_id}", {
      params: { path: { id: sessionId, item_id: itemId }, query: raw ? { raw: true } : {} },
    }),
  )
}

function ToolBody({
  host,
  sessionId,
  step,
  full,
}: {
  host: Host
  sessionId: string
  step: ToolStep
  full: boolean
}) {
  const [raw, setRaw] = useState(false)
  const detail = useQuery({
    queryKey: qk.sessions.trajectoryItem(host, sessionId, step.id, step.detail_rev, false),
    queryFn: () => readDetail(host, sessionId, step.id, false),
    placeholderData: keepPreviousData,
  })
  const rawRead = useQuery({
    queryKey: qk.sessions.trajectoryItem(host, sessionId, step.id, step.detail_rev, true),
    queryFn: () => readDetail(host, sessionId, step.id, true),
    placeholderData: keepPreviousData,
    enabled: raw,
  })
  return (
    <div className="space-y-1.5">
      {detail.data ? (
        <DetailView host={host} detail={detail.data.detail} />
      ) : detail.isError ? (
        <ReadError what="the detail" error={detail.error} />
      ) : (
        <Loading what="the detail" />
      )}
      {raw ? (
        rawRead.data ? (
          <>
            <Section name="raw input" code={json(rawRead.data.raw?.input)} language="json" />
            <Section name="raw output" code={json(rawRead.data.raw?.output)} language="json" />
          </>
        ) : rawRead.isError ? (
          <ReadError what="the raw data" error={rawRead.error} />
        ) : (
          <Loading what="the raw data" />
        )
      ) : null}
      <Controls step={step} full={full} raw={raw} onRaw={() => setRaw((was) => !was)} />
    </div>
  )
}

function json(value: unknown): string {
  return value === undefined ? "null" : JSON.stringify(value, null, 2)
}

function DetailView({ host, detail }: { host: Host; detail: ToolDetail }) {
  switch (detail.type) {
    case "command":
      return (
        <>
          <Section name="command" code={detail.command} language="shell" />
          {detail.cwd || (detail.exit_code !== null && detail.exit_code !== undefined) ? (
            <p className="px-2 font-mono text-xs text-muted-foreground">
              {detail.cwd ? <span>in {detail.cwd}</span> : null}
              {detail.cwd && detail.exit_code != null ? <span aria-hidden> · </span> : null}
              {detail.exit_code != null ? <span>exit code {detail.exit_code}</span> : null}
            </p>
          ) : null}
          {detail.output ? (
            <Section name={detail.truncated ? "output (its end)" : "output"} code={detail.output} />
          ) : null}
        </>
      )
    case "diff":
      return (
        <>
          {detail.files.map((file) => (
            <DiffFileView key={file.path} file={file} />
          ))}
        </>
      )
    case "file":
      return detail.text != null ? (
        <Section
          name={detail.path ?? "file"}
          code={detail.text}
          language={detail.language ?? undefined}
          firstLine={detail.line ?? 1}
        />
      ) : (
        <Empty />
      )
    case "search":
      return (
        <>
          {detail.query ? <Section name="query" code={detail.query} /> : null}
          {detail.text ? <Section name="result" code={detail.text} /> : null}
        </>
      )
    case "fetch":
      return (
        <>
          {detail.url ? <Section name="url" code={detail.url} /> : null}
          {detail.text ? <Section name="result" code={detail.text} /> : null}
        </>
      )
    case "generic": {
      const text = contentText(detail.content)
      if (!text && detail.content.length === 0) return <Empty />
      return (
        <>
          {text ? <Section name="output" code={text} /> : null}
          <MediaBlocks host={host} blocks={detail.content} />
        </>
      )
    }
  }
}

/** One code section: its own frame, its name, its language and its Copy. */
function Section({
  name,
  code,
  language,
  firstLine,
}: {
  name: string
  code: string
  language?: string
  firstLine?: number
}) {
  return (
    <div data-section={name} className="rounded-md border border-border/60 px-2 pt-0.5 pb-1.5">
      <CodeBlock caption={name} code={code} language={language} firstLine={firstLine} />
    </div>
  )
}

/** The change of one file in the task diff viewer, under one frame with its path and its Copy. */
function DiffFileView({ file }: { file: DiffFileDetail }) {
  const parsed = useMemo(
    () => parseUnifiedDiff(`diff --git a/${file.path} b/${file.path}\n${file.unified}`).files[0],
    [file.path, file.unified],
  )
  if (!parsed || parsed.hunks.length === 0)
    return (
      <div
        data-section={file.path}
        className="rounded-md border border-border/60 px-2 pt-0.5 pb-1.5"
      >
        <CodeBlock caption={file.path} code={file.unified} diff />
      </div>
    )
  const copy = `Copy the diff of ${file.path}`
  return (
    <div data-section={file.path} className="overflow-hidden rounded-md border border-border/60">
      <div className="flex items-center gap-2 px-2 py-0.5 font-mono text-[11px] text-muted-foreground">
        <span className="min-w-0 flex-1 truncate">
          {file.path}
          {file.language ? <span aria-hidden> · </span> : null}
          {file.language ? <span>{languageLabel(file.language)}</span> : null}
        </span>
        <Button
          type="button"
          size="icon-xs"
          variant="ghost"
          className="size-5"
          aria-label={copy}
          title={copy}
          onClick={() =>
            void copyText(file.unified).then((copied) =>
              copied ? toast.success("Diff copied") : toast.error("Could not copy the diff"),
            )
          }
        >
          <CopyIcon />
        </Button>
      </div>
      <DiffEditor file={parsed} wrap />
    </div>
  )
}

/** The steps of a subagent, each a row that opens in place. */
function readSubagentSteps(host: Host, sessionId: string, step: SubagentStep, before?: string) {
  return unwrap(
    api(host).GET("/v1/sessions/{id}/trajectory/turns/{turn_id}/steps", {
      params: {
        path: { id: sessionId, turn_id: step.turn_id },
        query: {
          subagent: step.subagent_id,
          limit: SUBAGENT_STEPS,
          ...(before === undefined ? {} : { before }),
        },
      },
    }),
  )
}

/** The older pages of a subagent's steps that `Show N earlier` read, and how many remain before them. */
interface OlderSteps {
  steps: StepDto[]
  earlier: number
}

/**
 * The steps of a subagent, each a row that opens in place. The newest page
 * is read again at each new step of the subagent, and the live version of a
 * step that the stream brought wins over the page's, so a running child
 * call shows its new `detail_rev`. `Show N earlier` reads the page before
 * the first row.
 */
function SubagentBody({
  host,
  sessionId,
  step,
  now,
  full,
}: {
  host: Host
  sessionId: string
  step: SubagentStep
  now: string
  full: boolean
}) {
  const live = useChildSteps()
  const newest = useQuery({
    queryKey: qk.sessions.subagentSteps(
      host,
      sessionId,
      step.turn_id,
      step.subagent_id,
      step.activity.steps,
    ),
    queryFn: () => readSubagentSteps(host, sessionId, step),
    placeholderData: keepPreviousData,
  })
  const [older, setOlder] = useState<OlderSteps | null>(null)
  const [olderLoad, setOlderLoad] = useState<{ loading: boolean; error?: unknown }>({
    loading: false,
  })
  // Each newest page read stays: a step that a newer page moves past stays a row.
  const [held, setHeld] = useState<StepDto[]>([])
  useEffect(() => {
    if (newest.data) setHeld((now) => mergeById(now, newest.data.steps))
  }, [newest.data])
  const read = useMemo(
    () => mergeById(older?.steps ?? [], mergeById(held, newest.data?.steps ?? [])),
    [older, held, newest.data],
  )
  const steps = useMemo(
    () => withLive(read, childSteps(live, step.turn_id, step.subagent_id)),
    [read, live, step.turn_id, step.subagent_id],
  )
  // The daemon counts the steps before the first of its page: a row shown
  // before that one is not still to read.
  const base = older
    ? { first: older.steps[0]?.id, earlier: older.earlier }
    : {
        first: newest.data?.steps[0]?.id,
        earlier: newest.data?.earlier_steps ?? 0,
      }
  const before = base.first
  const shownBefore = before === undefined ? 0 : steps.filter((one) => one.id < before).length
  const earlier = Math.max(0, base.earlier - shownBefore)

  async function loadEarlier() {
    setOlderLoad({ loading: true })
    try {
      const page = await readSubagentSteps(host, sessionId, step, steps[0]?.id)
      setOlder((held) => ({
        steps: [...page.steps, ...(held?.steps ?? [])],
        earlier: page.earlier_steps,
      }))
      setOlderLoad({ loading: false })
    } catch (error) {
      setOlderLoad({ loading: false, error })
    }
  }

  return (
    <div className="space-y-1.5">
      {step.task ? (
        <div className="rounded-md border border-border/60 px-2 py-1 text-sm">
          <Markdown>{step.task}</Markdown>
        </div>
      ) : null}
      {newest.data ? (
        <>
          {earlier > 0 ? (
            <Button
              size="xs"
              variant="ghost"
              disabled={olderLoad.loading}
              onClick={() => void loadEarlier()}
              className="font-mono text-muted-foreground"
            >
              Show {earlier} earlier
            </Button>
          ) : null}
          {olderLoad.error ? <ReadError what="the earlier steps" error={olderLoad.error} /> : null}
          {steps.length > 0 ? (
            <ul aria-label={`Steps of ${step.title}`} className="space-y-0.5">
              {steps.map((one) => (
                <li key={one.id}>
                  <StepDetail host={host} sessionId={sessionId} step={one} now={now} />
                </li>
              ))}
            </ul>
          ) : (
            <p className="px-2 font-mono text-xs text-muted-foreground">No steps yet.</p>
          )}
        </>
      ) : newest.isError ? (
        <ReadError what="the steps" error={newest.error} />
      ) : (
        <Loading what="the steps" />
      )}
      <Controls step={step} full={full} />
    </div>
  )
}

/** The steps of both lists, each once, oldest first: the version in `newer` wins. */
function mergeById(older: readonly StepDto[], newer: readonly StepDto[]): StepDto[] {
  const byId = new Map<string, StepDto>()
  for (const one of older) byId.set(one.id, one)
  for (const one of newer) byId.set(one.id, one)
  return [...byId.values()].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
}

/**
 * The steps read, at their live version where the stream has one. A live
 * step newer than the first row read joins them. An older one belongs to an
 * earlier page, which `Show N earlier` reads.
 */
function withLive(read: StepDto[], live: StepDto[]): StepDto[] {
  const first = read[0]?.id
  return mergeById(
    read,
    live.filter((one) => first === undefined || one.id >= first),
  )
}

function CancelSubagent({
  host,
  sessionId,
  step,
}: {
  host: Host
  sessionId: string
  step: SubagentStep
}) {
  const [pending, setPending] = useState(false)
  return (
    <Button
      size="xs"
      variant="ghost"
      disabled={pending}
      aria-label={`Cancel the subagent ${step.title}`}
      onClick={() => {
        setPending(true)
        unwrap(
          api(host).POST("/v1/sessions/{id}/console/cancel", {
            params: { path: { id: sessionId } },
            body: { subagent_id: step.subagent_id },
          }),
        )
          .catch((error) =>
            toast.error("Could not cancel the subagent", { description: describeError(error) }),
          )
          .finally(() => setPending(false))
      }}
    >
      Cancel
    </Button>
  )
}

/** The detail of a step that the trajectory carries whole. */
function PlainBody({
  host,
  step,
}: {
  host: Host
  step: Exclude<StepDto, ToolStep | SubagentStep>
}) {
  switch (step.type) {
    case "message":
    case "thought":
      return (
        <div className="text-sm">
          <Markdown>{contentText(step.content)}</Markdown>
          <MediaBlocks host={host} blocks={step.content} />
        </div>
      )
    case "permission":
      return (
        <>
          <p className="px-2 text-sm">{step.question}</p>
          {step.tool.subject ? <Section name="call" code={step.tool.subject} /> : null}
          {step.answer ? (
            <p className="px-2 font-mono text-xs text-muted-foreground">
              answered {step.answer.label}
            </p>
          ) : null}
        </>
      )
    case "question":
      return (
        <div className="px-2 text-sm">
          <Markdown>{step.question}</Markdown>
          {step.options.length > 0 ? (
            <ul className="mt-1 list-disc pl-5 text-xs text-muted-foreground">
              {step.options.map((option) => (
                <li key={option.id}>{option.label}</li>
              ))}
            </ul>
          ) : null}
        </div>
      )
    case "note":
      return <Section name={step.severity} code={step.text} />
    case "error":
      return (
        <>
          <Section name="error" code={step.text} />
          {step.diagnosis ? <Section name="diagnosis" code={step.diagnosis} /> : null}
        </>
      )
    case "compaction":
      return step.error ? <Section name="error" code={step.error} /> : <Empty />
  }
}

function Loading({ what }: { what: string }) {
  return <p className="px-2 font-mono text-xs text-muted-foreground">Loading {what}…</p>
}

function Empty() {
  return <p className="px-2 font-mono text-xs text-muted-foreground">Nothing to show.</p>
}

function ReadError({ what, error }: { what: string; error: unknown }) {
  return (
    <p role="alert" className="px-2 font-mono text-xs text-status-danger-fg">
      Could not read {what}: {describeError(error)}
    </p>
  )
}
