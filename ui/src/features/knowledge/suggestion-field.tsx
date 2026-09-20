/**
 * The one field every filter of the knowledge screen is typed into (022): a
 * text input with the values it could take listed under it.
 *
 * It is Base UI's Autocomplete rather than an HTML `<datalist>`: the Tauri
 * WebKit window does not draw a datalist's own list reliably, and a list of
 * rows the app draws itself can say what each value is and where it lives.
 *
 * The list only suggests. Free text stays valid, the field keeps the label,
 * the URL parameter and the submit it always had, and what is typed is what
 * is applied unless a row is taken. Down and Up move through the rows, Enter
 * takes the active one, and Escape closes the list and keeps the text.
 *
 * Two modes, one per kind of value: {@link SymbolField} asks `search` for the
 * symbols under what is typed, and {@link PathField} takes the directories
 * and the files of the ref out of the file graph the Files tab already reads.
 */

import { Autocomplete } from "@base-ui/react/autocomplete"
import { useQuery } from "@tanstack/react-query"
import { useDeferredValue, useMemo, useState } from "react"

import { Input } from "@/components/ui/input"
import { cn } from "@/lib/format"

import { knowledgeGraphQueryOptions, knowledgeSearchQueryOptions } from "./queries"

/** At most this many paths are suggested, directories before files. */
const PATH_ROWS = 20

interface Suggestion {
  /** What the field takes when the row is taken. */
  value: string
  /** What the value is: a symbol's kind, or `directory` and `file` for a path. */
  kind: string
  /** Where the value is, where that is not the value itself. */
  where?: string
}

interface FieldProps {
  /** The field's name, which is its `aria-label` and its placeholder. */
  label: string
  repositoryId: string
  gitRef: string
  value: string
  onChange: (value: string) => void
  placeholder?: string
  /** What sizes the field, since the input itself fills what it is given. */
  className?: string
  inputClassName?: string
}

/** A field whose values are the symbols `search` finds under what is typed. */
export function SymbolField({ repositoryId, gitRef, value, ...field }: FieldProps) {
  // The query follows a beat behind the keystrokes, so typing keeps its pace.
  const typed = useDeferredValue(value.trim())
  const hits = useQuery(knowledgeSearchQueryOptions(repositoryId, gitRef, { q: typed }))
  const suggestions = useMemo(
    () => (hits.data ?? []).map((hit) => ({ value: hit.name, kind: hit.kind, where: hit.path })),
    [hits.data],
  )

  return <SuggestionField {...field} value={value} suggestions={suggestions} />
}

/**
 * A field whose values are the paths of the ref: every directory that holds
 * something, then every file, of those that contain what is typed.
 *
 * The paths come from the `graph` query, the same cache entry the Files tab
 * draws itself from, so the tab's own filter costs no request. It is read
 * only once something is typed: a tab with nothing typed into its path field
 * asks the daemon for no graph of its own.
 */
export function PathField({
  repositoryId,
  gitRef,
  value,
  limit,
  ...field
}: FieldProps & { limit?: number }) {
  const typed = useDeferredValue(value.trim())
  const graph = useQuery({
    ...knowledgeGraphQueryOptions(repositoryId, gitRef, limit),
    enabled: typed.length > 0,
  })
  const paths = graph.data?.nodes
  const suggestions = useMemo(
    () => pathSuggestions(paths?.map((node) => node.path) ?? [], typed),
    [paths, typed],
  )

  return <SuggestionField {...field} value={value} suggestions={suggestions} />
}

/**
 * The directories and the files that contain `typed`, directories first and
 * each half in path order, at most {@link PATH_ROWS} rows.
 *
 * A directory is every prefix of a file's path, so `src/api/routes.ts`
 * suggests `src/` and `src/api/` as well as itself — the filters this field
 * feeds all match on the text of a path, and a directory is how a whole
 * subtree is named in one.
 */
function pathSuggestions(paths: string[], typed: string): Suggestion[] {
  if (typed === "") return []
  const wanted = typed.toLowerCase()
  const directories = new Set<string>()
  for (const path of paths) {
    const segments = path.split("/")
    for (let end = 1; end < segments.length; end++) {
      directories.add(`${segments.slice(0, end).join("/")}/`)
    }
  }
  const matching = (values: Iterable<string>) =>
    [...values].filter((value) => value.toLowerCase().includes(wanted)).sort()

  return [
    ...matching(directories).map((value) => ({ value, kind: "directory" })),
    ...matching(paths).map((value) => ({ value, kind: "file" })),
  ].slice(0, PATH_ROWS)
}

/**
 * The input and its list: the same keyboard and the same look in every mode.
 *
 * The list is drawn under the field itself rather than in Base UI's own
 * popup. A popup is a layer: it takes the rest of the form out of the
 * accessibility tree while it is up, which is the wrong trade for a filter
 * the user types beside the button that applies it.
 *
 * That leaves this component to say when the list is up — rows to show, the
 * field focused, and no Escape since the last keystroke — and Base UI to do
 * the rest: the rows, the arrow keys, the active row and what Enter takes.
 */
function SuggestionField({
  label,
  value,
  onChange,
  suggestions,
  placeholder,
  className,
  inputClassName,
}: Omit<FieldProps, "repositoryId" | "gitRef"> & { suggestions: Suggestion[] }) {
  const [focused, setFocused] = useState(false)
  const [dismissed, setDismissed] = useState(false)
  const open = focused && !dismissed && suggestions.length > 0

  return (
    <Autocomplete.Root
      // The rows are already the answer to what is typed: symbol mode asked
      // the daemon for them, path mode matched them itself.
      mode="none"
      inline
      open={open}
      items={suggestions}
      value={value}
      itemToStringValue={(item: Suggestion) => item.value}
      onValueChange={(next, details) => {
        // Escape empties a Base UI autocomplete. Here the field keeps what
        // was typed: the list is what Escape closes.
        if (details.reason === "escape-key") return
        setDismissed(false)
        onChange(next)
      }}
    >
      <div className={cn("relative", className)}>
        <Autocomplete.Input
          // A row keeps the caret where it is — Base UI takes the pointer
          // off an item before the field could lose it — so the field's own
          // focus is what says whether the list belongs on screen.
          onFocus={() => setFocused(true)}
          onBlur={() => setFocused(false)}
          onKeyDown={(event) => {
            if (event.key === "Escape" && open) setDismissed(true)
          }}
          render={
            <Input
              aria-label={label}
              placeholder={placeholder ?? label}
              className={cn("w-full", inputClassName)}
              autoComplete="off"
              spellCheck={false}
            />
          }
        />
        {open ? (
          <Autocomplete.List className="absolute top-full left-0 z-50 mt-1 max-h-72 w-full min-w-56 overflow-y-auto rounded-lg border bg-popover p-1 text-popover-foreground shadow-md">
            {(item: Suggestion, index: number) => (
              <Autocomplete.Item
                key={`${item.value}:${item.where ?? ""}:${index}`}
                value={item}
                // The list is Base UI's; taking a row is this field's, since
                // an inline list has no popup for Base UI to fill from.
                onClick={() => {
                  onChange(item.value)
                  setDismissed(true)
                }}
                className="flex cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-sm select-none data-highlighted:bg-muted data-highlighted:text-foreground"
              >
                <span className="min-w-0 flex-1 truncate font-mono">{item.value}</span>
                <span className="shrink-0 text-xs text-muted-foreground">{item.kind}</span>
                {item.where ? (
                  <span className="max-w-[45%] truncate font-mono text-xs text-muted-foreground">
                    {item.where}
                  </span>
                ) : null}
              </Autocomplete.Item>
            )}
          </Autocomplete.List>
        ) : null}
      </div>
    </Autocomplete.Root>
  )
}
