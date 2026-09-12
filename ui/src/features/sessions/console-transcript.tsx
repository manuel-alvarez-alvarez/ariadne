/**
 * An `acp` session's console, read the way its events actually happened: a
 * chat rather than a log.
 *
 * The console carries no fixed list of event kinds (`console.rs`), so this
 * gives a handful of them — the ones the runtime reports today — a shape a
 * reader recognizes at a glance, and folds `permission.replied` into the
 * `permission_request` it answers rather than showing it as a line of its
 * own. Anything else, present or future — a message chunk, a thought, a
 * plan — falls back to the same one-line-plus-payload row `SessionActivity`
 * already gives every event, so nothing the runtime starts reporting next
 * goes unrepresented.
 */

import { ChevronRightIcon, OctagonXIcon, SquareIcon, WrenchIcon } from "lucide-react"
import { useState } from "react"

import type { AgentEventDto } from "@/api"
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { When } from "@/components/when"
import { cn } from "@/lib/format"

/**
 * How a still-open `permission_request` is answered, and what the console
 * already knows about one that is not open any more.
 *
 * `resolvedPromptText` is the other half of the same idea for
 * `user_prompt_submit`: the text the console itself posted, kept by the
 * container across the FIFO match to the event that confirms it (see
 * `acp-console.tsx`) — the payload's own `prompt` field carries the agent's
 * whole system prompt in front of it, which is not what a reader typed.
 */
export interface ConsoleContext {
  resolvedPromptText: ReadonlyMap<string, string>
  /** `option_id` the request was answered with, or `null` for "cancelled". */
  resolvedPermissions: ReadonlyMap<string, string | null>
  /** The `permission_request` event a submitted answer is still in flight for. */
  answeringId: string | null
  onAnswerPermission: (event: AgentEventDto, optionId: string, label: string) => void
}

/** One permission option, as the runtime reports it. */
interface PermissionOption {
  optionId: string
  name: string
}

export function ConsoleTranscript({
  events,
  context,
}: {
  events: AgentEventDto[]
  context: ConsoleContext
}) {
  // permission.replied carries no id back to the request it answers — the
  // FIFO pairing that resolves one is the container's, and by the time it
  // has run every reply is already folded into its request's own row.
  const visible = events.filter((event) => event.kind !== "permission.replied")
  return (
    <ol className="flex flex-col gap-2">
      {visible.map((event) => (
        <li key={event.id}>
          <ConsoleEventRow event={event} context={context} />
        </li>
      ))}
    </ol>
  )
}

function ConsoleEventRow({ event, context }: { event: AgentEventDto; context: ConsoleContext }) {
  switch (event.kind) {
    case "user_prompt_submit":
      return <TextBubble who="you" text={context.resolvedPromptText.get(event.id)} event={event} />
    case "agent_message":
      return <TextBubble who="agent" text={nonEmptyString(event.payload, "text")} event={event} />
    case "stop":
      return <SystemNote>Turn ended</SystemNote>
    case "pre_tool_use":
      return <ToolRow event={event} state="running" />
    case "post_tool_use":
      return (
        <ToolRow
          event={event}
          state={nonEmptyString(readAcp(event.payload), "status") === "failed" ? "failed" : "done"}
        />
      )
    case "permission_request":
      return <PermissionCard event={event} context={context} />
    case "session_start":
      return <SystemNote>Session started</SystemNote>
    case "session_end":
      return <SystemNote>Session ended</SystemNote>
    case "compaction_update":
      return <SystemNote>Conversation compacted</SystemNote>
    case "session.error":
      return (
        <Alert variant="destructive">
          <OctagonXIcon />
          <AlertTitle>The agent reported an error</AlertTitle>
          <AlertDescription>{event.summary}</AlertDescription>
        </Alert>
      )
    default:
      return <RawEventRow event={event} />
  }
}

/** A user or agent turn: the text it ended on, plain, in a chat bubble. */
function TextBubble({
  who,
  text,
  event,
}: {
  who: "you" | "agent"
  text: string | undefined
  event: AgentEventDto
}) {
  if (!text) {
    // A turn with nothing to show for it — no text typed, or the agent
    // stopped without saying anything — is still worth marking on the line.
    return who === "you" ? <RawEventRow event={event} /> : <SystemNote>Turn ended</SystemNote>
  }
  return (
    <div className={cn("flex", who === "you" && "justify-end")}>
      <div
        className={cn(
          "max-w-[85%] rounded-lg px-3 py-2 text-sm whitespace-pre-wrap",
          who === "you" ? "bg-primary text-primary-foreground" : "bg-card border",
        )}
      >
        <p className="mb-1 text-xs font-medium opacity-70">{who === "you" ? "You" : "Agent"}</p>
        {text}
      </div>
    </div>
  )
}

function ToolRow({ event, state }: { event: AgentEventDto; state: "running" | "done" | "failed" }) {
  return (
    <div className="flex items-center gap-2 rounded-lg border bg-card px-3 py-1.5 text-sm">
      <WrenchIcon
        className={cn(
          "size-3.5 shrink-0",
          state === "running" && "animate-pulse text-muted-foreground",
          state === "failed" && "text-destructive",
        )}
      />
      <span className="min-w-0 flex-1 truncate font-mono text-xs">{event.summary}</span>
      {state === "running" ? (
        <Badge variant="secondary" className="shrink-0">
          Running
        </Badge>
      ) : state === "failed" ? (
        <Badge variant="destructive" className="shrink-0">
          Failed
        </Badge>
      ) : null}
    </div>
  )
}

function PermissionCard({ event, context }: { event: AgentEventDto; context: ConsoleContext }) {
  const options = permissionOptions(event.payload)
  const resolved = context.resolvedPermissions.get(event.id)
  const toolName = nonEmptyString(event.payload, "tool_name") ?? event.summary
  const answering = context.answeringId === event.id

  return (
    <div className="rounded-lg border bg-card px-3 py-2 text-sm">
      <p className="mb-2 font-medium">
        Permission requested: <span className="font-mono text-xs">{toolName}</span>
      </p>
      {resolved !== undefined ? (
        <p className="text-xs text-muted-foreground">
          {resolved === null
            ? "Cancelled — nothing to select."
            : `Answered: ${options.find((option) => option.optionId === resolved)?.name ?? resolved}`}
        </p>
      ) : (
        <div className="flex flex-wrap gap-2">
          {options.map((option) => (
            <Button
              key={option.optionId}
              size="xs"
              variant="outline"
              pending={answering}
              disabled={answering}
              onClick={() => context.onAnswerPermission(event, option.optionId, option.name)}
            >
              {option.name}
            </Button>
          ))}
        </div>
      )}
    </div>
  )
}

function SystemNote({ children }: { children: string }) {
  return (
    <p className="flex items-center gap-1.5 py-1 text-center text-xs text-muted-foreground">
      <SquareIcon className="size-2.5" />
      {children}
    </p>
  )
}

/** The fallback every event kind gets until it earns a shape of its own. */
function RawEventRow({ event }: { event: AgentEventDto }) {
  const [open, setOpen] = useState(false)
  return (
    <div className="py-0.5">
      <button
        type="button"
        className="flex w-full items-baseline gap-2 text-left text-sm"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
      >
        <ChevronRightIcon
          className={cn(
            "size-3 shrink-0 translate-y-0.5 text-muted-foreground transition-transform",
            open && "rotate-90",
          )}
        />
        <Badge variant="secondary" className="shrink-0 font-mono">
          {event.kind}
        </Badge>
        <span className="min-w-0 flex-1 truncate font-mono text-xs text-muted-foreground">
          {event.summary}
        </span>
        <When
          at={event.created_at}
          format="age"
          label="reported"
          className="shrink-0 text-xs text-muted-foreground tabular-nums"
        />
      </button>
      {open ? (
        <section
          aria-label={`${event.kind} payload`}
          // biome-ignore lint/a11y/noNoninteractiveTabindex: a scroll container has to take focus to be scrollable by keyboard
          tabIndex={0}
          className="mt-1 max-h-64 overflow-auto rounded-md bg-muted p-2 focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
        >
          <pre className="font-mono text-xs">{stringify(event.payload)}</pre>
        </section>
      ) : null}
    </div>
  )
}

function nonEmptyString(payload: unknown, field: string): string | undefined {
  if (typeof payload !== "object" || payload === null) return undefined
  const value = (payload as Record<string, unknown>)[field]
  return typeof value === "string" && value.length > 0 ? value : undefined
}

function readAcp(payload: unknown): unknown {
  if (typeof payload !== "object" || payload === null) return undefined
  return (payload as Record<string, unknown>).acp
}

function permissionOptions(payload: unknown): PermissionOption[] {
  if (typeof payload !== "object" || payload === null) return []
  const options = (payload as Record<string, unknown>).options
  if (!Array.isArray(options)) return []
  return options.flatMap((option) => {
    if (typeof option !== "object" || option === null) return []
    const optionId = (option as Record<string, unknown>).optionId
    const name = (option as Record<string, unknown>).name
    if (typeof optionId !== "string") return []
    return [{ optionId, name: typeof name === "string" && name.length > 0 ? name : optionId }]
  })
}

function stringify(payload: unknown): string {
  try {
    return JSON.stringify(payload, null, 2) ?? String(payload)
  } catch {
    return String(payload)
  }
}
