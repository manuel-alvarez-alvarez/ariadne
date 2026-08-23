/**
 * The compose box under a conversation — `ariadne task msg` for the web, and
 * its goal-thread sibling. The daemon records the post as the user (no agent
 * session header, see `http/auth.rs`) and the message simply lands in the
 * thread for the agents to read when they next act — it accepts one whatever
 * state the goal or task is in, so the box never goes away.
 *
 * A message may name one addressee, the web's half of `--to`: the picker next
 * to Send offers whoever the thread's own surface says may be addressed, and
 * defaults to nobody — a message with no addressee goes to the thread, which is
 * what most of them are. The user is never in that list: they are the one
 * writing here.
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
import { useMemo, useRef, useState } from "react"

import type { CreateMessageRequest, MessageDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
import { matchesShortcut, shortcutLabel } from "@/lib/shortcuts"
import { cn } from "@/lib/utils"

const SEND: { key: string } = { key: "Enter" }

/** What the picker calls "addressed to no one", as option and as placeholder. */
const NOBODY = "the thread"

/** One profile the thread may address, as the picker offers it. */
export interface Addressee {
  /** Posted as `to`; the daemon takes a profile id or its name. */
  id: string
  name: string
}

export function MessageComposer({
  post,
  label,
  placeholder,
  addressees,
  className,
}: {
  /** The thread's `usePost…Message` mutation; its error is drawn inline. */
  post: UseMutationResult<MessageDto, Error, CreateMessageRequest>
  /** What the box is, for the accessibility tree. */
  label: string
  placeholder: string
  /**
   * Who this thread may address, in the daemon's own order (see
   * `http/recipients.rs`). Empty or absent, the box has no picker at all.
   */
  addressees?: readonly Addressee[]
  /** The surface's background — the box has to cover what scrolls under it. */
  className?: string
}) {
  const [draft, setDraft] = useState("")
  const [addressed, setAddressed] = useState<string | null>(null)
  const form = useRef<HTMLFormElement>(null)
  const body = draft.trim()
  // Derived rather than trusted: a task gains and loses reviewers while its
  // panel is open, and an addressee that left the thread would be refused.
  const to = addressees?.some((addressee) => addressee.id === addressed) ? addressed : null
  // What the trigger shows for the value it holds; without it, the raw id.
  const items = useMemo(
    () => (addressees ?? []).map(({ id, name }) => ({ label: name, value: id })),
    [addressees],
  )

  async function send() {
    if (!body || post.isPending) return
    try {
      // `undefined`, not `null`: an unaddressed message posts the body alone,
      // exactly the request this box sent before there was anything to address.
      await post.mutateAsync({ body, to: to ?? undefined })
    } catch {
      // Drawn inline below; the draft stays for another try.
      return
    }
    // Clear exactly what was sent: typing that happened mid-flight survives.
    // The addressee stays — answering one agent takes more than one message,
    // and the picker says on its face who the next one goes to.
    setDraft((current) => (current.trim() === body ? "" : current))
    // The mutation has already appended the message to the cached thread;
    // give React a frame to lay it out, then bring the thread's end — the
    // sent message, with this box under it — into view. The scroll has to go
    // to the panel, not this form: stuck, the form's own rect is always in
    // view already, so `scrollIntoView` on it would move nothing.
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        const panel = scrollParent(form.current)
        panel?.scrollTo({ top: panel.scrollHeight, behavior: "smooth" })
      })
    })
  }

  return (
    <form
      ref={form}
      className={cn("sticky bottom-0 flex flex-col gap-2 bg-background py-1", className)}
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
        <div className="flex items-center gap-2">
          {addressees?.length ? (
            <Select
              value={to}
              onValueChange={(value) => setAddressed(value as string | null)}
              items={items}
            >
              <SelectTrigger size="sm" aria-label="Addressee" className="max-w-44 text-xs">
                <span className="text-muted-foreground">To</span>
                <SelectValue placeholder={NOBODY} />
              </SelectTrigger>
              <SelectContent>
                {/* Clearing it: the message goes to the thread, not to anyone. */}
                <SelectItem value={null}>{NOBODY}</SelectItem>
                {addressees.map((addressee) => (
                  <SelectItem key={addressee.id} value={addressee.id}>
                    {addressee.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          ) : null}
          <Button type="submit" size="sm" disabled={!body} pending={post.isPending}>
            <SendIcon />
            Send
          </Button>
        </div>
      </div>
    </form>
  )
}

/**
 * The scroll container the box is stuck to — the side panel's popup, or
 * whatever holds the thread elsewhere. `document.scrollingElement` when
 * nothing on the way up scrolls and the page itself is the container.
 */
function scrollParent(el: HTMLElement | null): Element | null {
  for (let node = el?.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node)
    if (overflowY === "auto" || overflowY === "scroll") return node
  }
  return document.scrollingElement
}
