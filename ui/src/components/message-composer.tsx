/**
 * The compose box under a conversation — `ariadne task msg` for the web, and
 * its goal-thread sibling. The daemon records the post as the user (no agent
 * session header, see `http/auth.rs`) and the message simply lands in the
 * thread for the agents to read when they next act — it accepts one whatever
 * state the goal or task is in, so the box never goes away.
 *
 * The box is sticky at the bottom of the panel's scroll, so a long thread can
 * be answered from wherever the user has scrolled to. Sending clears the
 * draft; a failure keeps it and shows the daemon's error right above the
 * button, cleared again on the next edit. ⌘/Ctrl+Enter sends, and as
 * everywhere else (see `@/lib/shortcuts`) either modifier fires — only the
 * printed hint picks a side.
 */

import type { UseMutationResult } from "@tanstack/react-query"
import { SendIcon } from "lucide-react"
import { useRef, useState } from "react"

import type { MessageDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { matchesShortcut, shortcutLabel } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"

const SEND: { key: string } = { key: "Enter" }

export function MessageComposer({
  post,
  label,
  placeholder,
  className,
}: {
  /** The thread's `usePost…Message` mutation; its error is drawn inline. */
  post: UseMutationResult<MessageDto, Error, string>
  /** What the box is, for the accessibility tree. */
  label: string
  placeholder: string
  /** The surface's background — the box has to cover what scrolls under it. */
  className?: string
}) {
  const [draft, setDraft] = useState("")
  // The end of the message list in flow, for scrolling a sent message into
  // view. The sticky form itself cannot be the target: its rect is wherever
  // it is stuck, which is always in view already.
  const anchor = useRef<HTMLDivElement>(null)
  const body = draft.trim()

  async function send() {
    if (!body || post.isPending) return
    try {
      await post.mutateAsync(body)
    } catch {
      // Drawn inline below; the draft stays for another try.
      return
    }
    // Clear exactly what was sent: typing that happened mid-flight survives.
    setDraft((current) => (current.trim() === body ? "" : current))
    // The mutation has already appended the message to the cached thread;
    // give React a frame to lay it out, then bring the list's end into view.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        anchor.current?.scrollIntoView({ block: "end", behavior: "smooth" })
      })
    })
  }

  return (
    <>
      <div ref={anchor} aria-hidden />
      <form
        className={cn("sticky bottom-0 flex flex-col gap-2 bg-background pt-1 pb-1", className)}
        onSubmit={(event) => {
          event.preventDefault()
          void send()
        }}
      >
        <Textarea
          value={draft}
          aria-label={label}
          placeholder={placeholder}
          onChange={(event) => {
            setDraft(event.target.value)
            // A failure from the last attempt is stale once the text changes.
            if (post.isError) post.reset()
          }}
          onKeyDown={(event) => {
            if (matchesShortcut(event, SEND)) {
              event.preventDefault()
              void send()
            }
          }}
        />
        {post.isError ? <ErrorState title="Could not send the message" error={post.error} /> : null}
        <div className="flex items-center justify-between gap-2">
          <span className="text-xs text-muted-foreground">{shortcutLabel(SEND)} to send</span>
          <Button type="submit" size="sm" disabled={!body} pending={post.isPending}>
            <SendIcon />
            Send
          </Button>
        </div>
      </form>
    </>
  )
}
