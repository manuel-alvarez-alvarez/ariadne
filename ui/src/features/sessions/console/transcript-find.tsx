import { useCallback, useEffect, useRef, useState } from "react"
import { Button } from "@/components/ui/button"

/** Search only the textual blocks represented by the current loaded view. */
export function TranscriptFind({
  entries,
  older,
  loading,
  loadOlder,
}: {
  entries: { id: string; text: string }[]
  older: boolean
  loading: boolean
  loadOlder: () => void
}) {
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState("")
  const [selected, setSelected] = useState("")
  const root = useRef<HTMLDivElement>(null)
  const input = useRef<HTMLInputElement>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const returnFocus = useRef<HTMLElement | null>(null)
  const matches = query
    ? entries.flatMap((entry) => {
        const result = []
        const text = entry.text.toLowerCase()
        const needle = query.toLowerCase()
        let offset = text.indexOf(needle)
        while (offset >= 0) {
          result.push({ ...entry, offset, key: `${entry.id}:${offset}` })
          offset = text.indexOf(needle, offset + needle.length)
        }
        return result
      })
    : []
  const index = Math.max(
    0,
    matches.findIndex((match) => match.key === selected),
  )
  const active = matches[index]
  function close() {
    setOpen(false)
    ;(returnFocus.current ?? trigger.current)?.focus()
  }
  const show = useCallback((target: HTMLElement | null) => {
    returnFocus.current = target
    setOpen(true)
  }, [])
  function move(direction: number) {
    if (matches.length)
      setSelected(matches[(index + direction + matches.length) % matches.length]?.key ?? "")
  }
  useEffect(() => {
    const console = root.current?.closest("[data-console]")
    if (!console) return
    function key(event: Event) {
      const e = event as KeyboardEvent
      const target = e.target as HTMLElement
      if (e.key.toLowerCase() !== "f" || !(e.ctrlKey || e.metaKey) || e.altKey) return
      if (target.closest('input,textarea,select,[contenteditable="true"],[role="textbox"]')) return
      if (target.closest('[role="dialog"]') !== console?.closest('[role="dialog"]')) return
      e.preventDefault()
      e.stopPropagation()
      show(target)
    }
    console.addEventListener("keydown", key)
    return () => console.removeEventListener("keydown", key)
  }, [show])
  useEffect(() => {
    if (open) input.current?.focus()
  }, [open])
  // Pin the first result by identity so prepended history cannot select another result.
  useEffect(() => {
    if (active && !matches.some((match) => match.key === selected)) setSelected(active.key)
  }, [active, matches, selected])
  return (
    <div ref={root} className="relative border-b px-3 py-1">
      <Button ref={trigger} size="xs" variant="ghost" onClick={() => show(trigger.current)}>
        Find in transcript
      </Button>
      {open ? (
        <search
          aria-label="Find loaded transcript"
          className="absolute inset-x-0 top-full z-20 max-h-96 space-y-2 overflow-auto border bg-background p-3 shadow-lg"
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              event.preventDefault()
              event.stopPropagation()
              close()
            }
            if (event.key === "Enter" && event.target === input.current) {
              event.preventDefault()
              move(event.shiftKey ? -1 : 1)
            }
          }}
        >
          <input
            ref={input}
            aria-label="Find in transcript"
            value={query}
            className="rounded border px-2 py-1"
            onChange={(event) => {
              setQuery(event.target.value)
              setSelected("")
            }}
          />
          <span role="status">
            {active ? `${index + 1} of ${matches.length}` : "No matches in loaded messages"}
          </span>
          <Button
            size="xs"
            variant="ghost"
            disabled={!active}
            onClick={() => move(-1)}
            aria-label="Previous match"
          >
            Previous
          </Button>
          <Button
            size="xs"
            variant="ghost"
            disabled={!active}
            onClick={() => move(1)}
            aria-label="Next match"
          >
            Next
          </Button>
          <Button size="xs" variant="ghost" onClick={close}>
            Close find
          </Button>
          <p className="text-xs text-muted-foreground">
            Searching loaded messages.{" "}
            {older ? "Older history remains unloaded." : "All available history is loaded."}
          </p>
          {older ? (
            <Button size="xs" variant="outline" disabled={loading} onClick={loadOlder}>
              Load earlier history
            </Button>
          ) : null}
          {active ? (
            <section
              aria-label="Matching transcript content"
              className="max-h-48 overflow-auto whitespace-pre-wrap break-words text-xs"
            >
              {active.text.slice(0, active.offset)}
              <mark>{active.text.slice(active.offset, active.offset + query.length)}</mark>
              {active.text.slice(active.offset + query.length)}
            </section>
          ) : null}
        </search>
      ) : null}
    </div>
  )
}
