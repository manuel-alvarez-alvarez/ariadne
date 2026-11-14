import { SendIcon, SquareIcon } from "lucide-react"
import type { KeyboardEvent, ReactNode, RefObject } from "react"
import { useLayoutEffect } from "react"

import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

import type { AgentCommand } from "./transcript"

/** The most commands the suggestion list shows. */
const SUGGESTIONS = 8

/** The lines the composer grows to before it scrolls inside. */
const COMPOSER_LINES = 6

/** The commands a typed `/word` names, while the cursor is still in that word. */
export function suggest(
  commands: AgentCommand[],
  draft: string,
  cursor: number,
): AgentCommand[] | undefined {
  if (commands.length === 0 || !draft.startsWith("/") || /\s/.test(draft.slice(0, cursor))) {
    return undefined
  }
  const typed = (draft.split(/\s/)[0] ?? "").slice(1).toLowerCase()
  const matching = commands.filter((command) => command.name.toLowerCase().includes(typed))
  const first = matching.filter((command) => command.name.toLowerCase().startsWith(typed))
  return [...first, ...matching.filter((command) => !first.includes(command))].slice(0, SUGGESTIONS)
}

function SuggestionList({
  commands,
  picked,
  onPick,
}: {
  commands: AgentCommand[]
  picked: number
  onPick: (command: AgentCommand) => void
}) {
  if (commands.length === 0) {
    return <p className="pt-1 font-mono text-xs text-muted-foreground">no matching command</p>
  }
  return (
    <div
      role="listbox"
      aria-label="Slash commands"
      className="mt-1 space-y-0.5 rounded-md border p-1 font-mono text-xs"
    >
      {commands.map((command, index) => (
        <div
          key={command.name}
          role="option"
          aria-selected={index === picked}
          tabIndex={-1}
          // A press must not take the keyboard from the composer, and a tap
          // or a click completes the command for editing.
          onPointerDown={(event) => event.preventDefault()}
          onClick={() => onPick(command)}
          onKeyDown={() => undefined}
          className={cn(
            "flex cursor-pointer items-baseline gap-2 rounded px-2 py-1.5 hover:bg-muted",
            index === picked && "bg-primary text-primary-foreground hover:bg-primary",
          )}
        >
          <span className="shrink-0">/{command.name}</span>
          <span className="min-w-0 truncate opacity-70">{command.description}</span>
          {command.hint ? <span className="shrink-0 opacity-70">{command.hint}</span> : null}
        </div>
      ))}
    </div>
  )
}

/**
 * The composer row: the menu, the message box with its suggestions, the Stop
 * and Send controls. A child's view has no input, so
 * there it is a line that says so.
 */
export function Composer({
  fieldRef,
  menu,
  draft,
  live,
  running,
  question,
  child,
  suggestions,
  picked,
  onChange,
  onSelect,
  onKeyDown,
  onPick,
  onSend,
  onStop,
}: {
  fieldRef: RefObject<HTMLTextAreaElement | null>
  /** The plus menu, drawn at the row's start. */
  menu: ReactNode
  draft: string
  live: boolean
  running: boolean
  /** A permission question waits for a pick. */
  /** A `wait_for_user` question waits for an answer. */
  question: boolean
  /** The child whose view is shown; none for `main`. */
  child: { cancellable: boolean } | undefined
  suggestions: AgentCommand[] | undefined
  picked: number
  onChange: (text: string, at: number) => void
  onSelect: (at: number) => void
  onKeyDown: (event: KeyboardEvent<HTMLTextAreaElement>) => void
  onPick: (command: AgentCommand) => void
  onSend: () => void
  onStop: () => void
}) {
  const readOnly = child !== undefined

  // The composer is one line, grows with its text to six, then scrolls inside.
  // biome-ignore lint/correctness/useExhaustiveDependencies: the draft is what changes the height
  useLayoutEffect(() => {
    const el = fieldRef.current
    if (!el) return
    const style = getComputedStyle(el)
    const line = Number.parseFloat(style.lineHeight) || 20
    const chrome =
      Number.parseFloat(style.paddingTop || "0") +
      Number.parseFloat(style.paddingBottom || "0") +
      Number.parseFloat(style.borderTopWidth || "0") +
      Number.parseFloat(style.borderBottomWidth || "0")
    const most = line * COMPOSER_LINES + chrome
    el.style.height = "auto"
    el.style.height = `${Math.min(el.scrollHeight, most)}px`
    el.style.overflowY = el.scrollHeight > most ? "auto" : "hidden"
  }, [draft, readOnly])

  if (child) {
    return (
      <div className="border-t px-3 py-2 font-mono text-xs text-muted-foreground">
        <div className="flex items-center gap-2">
          {menu}
          <span>
            read-only · arrows switch view · {child.cancellable ? "esc cancel" : "esc back"}
          </span>
        </div>
      </div>
    )
  }

  return (
    <div className="flex items-end gap-2 border-t p-2">
      {menu}
      <div className="min-w-0 flex-1">
        <textarea
          ref={fieldRef}
          aria-label="Message the agent"
          value={draft}
          disabled={!live}
          rows={1}
          placeholder={
            !live
              ? undefined
              : question
                ? "Answer the question. Enter sends your answer."
                : "Message the agent. Enter sends, Shift+Enter adds a line."
          }
          onChange={(event) => onChange(event.target.value, event.target.selectionStart)}
          onSelect={(event) => onSelect(event.currentTarget.selectionStart)}
          onKeyDown={onKeyDown}
          className="block w-full resize-none rounded-md border border-input bg-transparent px-2.5 py-1.5 text-sm outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50"
        />
        {live ? null : (
          <p className="pt-1 text-xs text-muted-foreground">
            This session has ended. Resume it to talk to it.
          </p>
        )}
        {suggestions ? (
          <SuggestionList commands={suggestions} picked={picked} onPick={onPick} />
        ) : null}
      </div>
      {running ? (
        <Tooltip>
          <TooltipTrigger
            render={
              <Button
                size="icon"
                variant="outline"
                onClick={onStop}
                aria-label="Stop the turn"
                className="shrink-0"
              />
            }
          >
            <SquareIcon />
          </TooltipTrigger>
          <TooltipContent>Stop the turn</TooltipContent>
        </Tooltip>
      ) : null}
      <Tooltip>
        <TooltipTrigger
          render={
            <Button
              size="icon"
              onClick={onSend}
              disabled={!live || draft.trim().length === 0}
              aria-label="Send"
              className="shrink-0"
            />
          }
        >
          <SendIcon />
        </TooltipTrigger>
        <TooltipContent>Send</TooltipContent>
      </Tooltip>
    </div>
  )
}
