/**
 * The compose box under a conversation — `ariadne task msg` for the web, and
 * its goal-thread sibling. The daemon records the post as the user (no agent
 * session header, see `http/auth.rs`) and the message simply lands in the
 * thread for the agents to read when they next act.
 *
 * A message may name one addressee, the web's half of `--to`: the picker next
 * to Send offers whoever the thread's own surface says may be addressed, and
 * defaults to nobody — a message with no addressee goes to the thread, which is
 * what most of them are. The user is never in that list: they are the one
 * writing here.
 *
 * A thread opened *to answer* an agent that is waiting — from the attention
 * list — arrives with {@link autoFocus} and {@link presetTo} set: the keyboard
 * is already in the box and the picker already names whoever asked, so the
 * answer is one keystroke away rather than two clicks. Both are read on the
 * way in only; the picker is the user's from then on.
 *
 * The box closes on a goal or task that is over. The daemon would still take
 * the post — it checks only that the row exists — but there is no session left
 * to read it and none will be started, so a box that took the message anyway
 * would be a message written into nothing. It says which it is instead, and the
 * thread stays readable.
 *
 * Nothing typed here is lost by leaving. The draft is kept per thread (see
 * `thread-drafts.ts`) from the first keystroke, so a panel dismissed by an
 * outside press — or Escape, or a link out — comes back with the sentence
 * half-written, and only a message that actually posted clears it.
 *
 * Sending clears the draft; a failure keeps it and shows the daemon's error
 * right above the button, cleared again on the next edit. ⌘/Ctrl+Enter sends,
 * and as everywhere else (see `@/lib/shortcuts`) either modifier fires — only
 * the printed hint picks a side.
 */

import type { UseMutationResult } from "@tanstack/react-query"
import { SendIcon } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"

import type { CreateMessageRequest, MessageDto } from "@/api"
import { ErrorState } from "@/components/error-state"
import { readDraft, type ThreadKey, writeDraft } from "@/components/thread-drafts"
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
  draftKey,
  label,
  placeholder,
  addressees,
  autoFocus,
  presetTo,
  closedHint,
  onSent,
}: {
  /** The thread's `usePost…Message` mutation; its error is drawn inline. */
  post: UseMutationResult<MessageDto, Error, CreateMessageRequest>
  /** Which thread this box writes to, and whose draft it holds. */
  draftKey: ThreadKey
  /** What the box is, for the accessibility tree. */
  label: string
  placeholder: string
  /** Take the keyboard on arrival; see the note above. */
  autoFocus?: boolean
  /** Whom the picker starts on — an addressee id, read once on mount. */
  presetTo?: string | null
  /**
   * Who this thread may address, in the daemon's own order (see
   * `http/recipients.rs`). Empty or absent, the box has no picker at all.
   */
  addressees?: readonly Addressee[]
  /**
   * Why nothing can be written here any more — a goal or task that is over.
   * Set, the box is out of reach and this is shown in place of the send hint.
   */
  closedHint?: string
  /** A message posted; the thread brings the reader to it. */
  onSent?: () => void
}) {
  const [draft, setDraft] = useState(() => readDraft(draftKey))
  // The preset is an initial value and nothing more: once the box is on
  // screen, who the next message goes to is the user's to change and not the
  // link's to keep re-asserting.
  const [addressed, setAddressed] = useState<string | null>(presetTo ?? null)
  const field = useRef<HTMLTextAreaElement>(null)
  // The panel can move to another thread without unmounting this box — the
  // stacked task panel does exactly that — and the draft on screen has to be
  // the one belonging to the thread now under it.
  const thread = useRef(draftKey)
  if (thread.current !== draftKey) {
    thread.current = draftKey
    setDraft(readDraft(draftKey))
  }

  // Once, on arrival. `autoFocus` on the element itself is the same thing
  // written where it also steals the keyboard from anything that re-renders
  // the box.
  useEffect(() => {
    if (autoFocus) field.current?.focus()
  }, [autoFocus])

  const body = draft.trim()
  // Derived rather than trusted: a task gains and loses reviewers while its
  // panel is open, and an addressee that left the thread would be refused.
  const to = addressees?.some((addressee) => addressee.id === addressed) ? addressed : null
  // What the trigger shows for the value it holds; without it, the raw id.
  const items = useMemo(
    () => (addressees ?? []).map(({ id, name }) => ({ label: name, value: id })),
    [addressees],
  )

  function edit(next: string) {
    setDraft(next)
    writeDraft(draftKey, next)
  }

  async function send() {
    if (!body || post.isPending || closedHint) return
    try {
      // `undefined`, not `null`: an unaddressed message posts the body alone,
      // exactly the request this box sent before there was anything to address.
      await post.mutateAsync({ body, to: to ?? undefined })
    } catch {
      // Drawn inline below; the draft stays for another try.
      return
    }
    // Clear exactly what was sent: typing that happened mid-flight survives,
    // in the box and in what is kept of it.
    setDraft((current) => {
      const left = current.trim() === body ? "" : current
      writeDraft(draftKey, left)
      return left
    })
    // The addressee stays — answering one agent takes more than one message,
    // and the picker says on its face who the next one goes to.
    onSent?.()
  }

  return (
    <form
      className="flex flex-col gap-2 py-1"
      onSubmit={(event) => {
        event.preventDefault()
        void send()
      }}
    >
      <Textarea
        ref={field}
        value={draft}
        aria-label={label}
        placeholder={placeholder}
        disabled={Boolean(closedHint)}
        onChange={(event) => {
          edit(event.target.value)
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
        <span className="text-xs text-muted-foreground">
          {closedHint ?? `${shortcutLabel(SEND)} to send`}
        </span>
        <div className="flex items-center gap-2">
          {addressees?.length && !closedHint ? (
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
          <Button
            type="submit"
            size="sm"
            disabled={!body || Boolean(closedHint)}
            pending={post.isPending}
          >
            <SendIcon />
            Send
          </Button>
        </div>
      </div>
    </form>
  )
}
