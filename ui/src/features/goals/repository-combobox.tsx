/**
 * The repositories field of the goal form: the registered checkouts, searched
 * rather than scrolled.
 *
 * The registry is unbounded, so the list this replaces — every repository as a
 * checkbox in a scroll box — stopped being readable well before it stopped
 * fitting. The pattern is `profiles/model-combobox.tsx` turned multi-select:
 * the same Base UI popover over a cmdk list, except the input lives *inside*
 * the popup (there is no free text here — a goal is created against ids), a
 * pick toggles instead of commits, and the popup stays open so several can be
 * made in a row.
 *
 * What is chosen stays visible with the popup closed: one chip per repository
 * in the field itself, each removable on its own, so the set never has to be
 * reopened to be read. The chips are ordered by when they were picked, which
 * is the order that goes on the wire.
 *
 * The trigger is a button and the chips' removes are buttons beside it rather
 * than inside it — nested buttons are not markup — which is also what lets the
 * field carry an ordinary `<label for>`.
 */

import { Popover } from "@base-ui/react/popover"
import { ChevronsUpDownIcon, XIcon } from "lucide-react"
import { useMemo, useRef, useState } from "react"

import type { RepositoryDto } from "@/api"
import { Badge } from "@/components/ui/badge"
import {
  Command,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"

export function RepositoryCombobox({
  id,
  repositories,
  value,
  onChange,
  invalid,
}: {
  /** The trigger's id, for the field's `<label for>`. */
  id: string
  /** Everything registered; the popup filters this, never the value. */
  repositories: RepositoryDto[]
  /** Picked ids, in pick order. */
  value: string[]
  onChange: (ids: string[]) => void
  invalid?: boolean
}) {
  const [open, setOpen] = useState(false)
  const fieldRef = useRef<HTMLDivElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)

  // Chips follow the value, not the catalog: an id the registry no longer has
  // simply has no chip, rather than a chip with nothing to say.
  const byId = useMemo(
    () => new Map(repositories.map((repository) => [repository.id, repository])),
    [repositories],
  )
  const picked = value.map((one) => byId.get(one)).filter((one) => one !== undefined)

  function toggle(repositoryId: string) {
    onChange(
      value.includes(repositoryId)
        ? value.filter((one) => one !== repositoryId)
        : [...value, repositoryId],
    )
  }

  return (
    <Popover.Root open={open} onOpenChange={setOpen} modal={false}>
      <div
        ref={fieldRef}
        data-invalid={invalid ? "" : undefined}
        className="flex min-h-8 w-full flex-wrap items-center gap-1 rounded-lg border border-input bg-transparent p-1 transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 data-invalid:border-destructive data-invalid:ring-3 data-invalid:ring-destructive/20 dark:bg-input/30 dark:data-invalid:border-destructive/50 dark:data-invalid:ring-destructive/40"
      >
        {picked.map((repository) => (
          <Badge
            key={repository.id}
            variant="secondary"
            className="max-w-full gap-1 pr-1 font-mono"
          >
            <span className="truncate">{repository.path}</span>
            <button
              type="button"
              aria-label={`Remove ${repository.path}`}
              onClick={() => toggle(repository.id)}
              className="rounded-full opacity-60 transition-opacity outline-none hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
            >
              <XIcon />
            </button>
          </Badge>
        ))}
        <Popover.Trigger
          id={id}
          role="combobox"
          aria-haspopup="listbox"
          aria-invalid={invalid ? true : undefined}
          // Filling the rest of the row keeps the whole field clickable, which
          // is what the box would do if it were the control it looks like.
          className="flex min-w-32 flex-1 items-center justify-between gap-2 rounded-md px-1.5 py-0.5 text-left text-sm text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:text-foreground"
        >
          {picked.length > 0 ? "Add another…" : "Select repositories…"}
          <ChevronsUpDownIcon className="size-4 shrink-0 opacity-50" />
        </Popover.Trigger>
      </div>
      <Popover.Portal>
        <Popover.Positioner
          anchor={fieldRef}
          side="bottom"
          align="start"
          sideOffset={4}
          className="isolate z-50"
        >
          <Popover.Popup
            initialFocus={searchRef}
            className="w-(--anchor-width) rounded-lg bg-popover text-popover-foreground shadow-md ring-1 ring-foreground/10 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
          >
            <Command label="Search repositories">
              <CommandInput ref={searchRef} placeholder="Search repositories…" />
              <CommandList
                label="Repositories"
                className="max-h-64"
                // A pick must not take focus off the search box, or the next
                // one would have to reach for it again.
                onMouseDown={(event) => event.preventDefault()}
              >
                <CommandEmpty>No matching repositories.</CommandEmpty>
                {repositories.map((repository) => {
                  const selected = value.includes(repository.id)
                  return (
                    <CommandItem
                      key={repository.id}
                      // Ids are what the form holds and paths are what is read,
                      // so the searchable text is spelled out as keywords.
                      value={repository.id}
                      keywords={[
                        repository.path,
                        repository.base_branch,
                        ...(repository.description ? [repository.description] : []),
                      ]}
                      data-checked={selected ? "true" : "false"}
                      onSelect={() => toggle(repository.id)}
                    >
                      <div className="flex min-w-0 flex-1 flex-col">
                        <span className="flex flex-wrap items-baseline gap-x-2">
                          <span className="truncate font-mono text-[13px]">{repository.path}</span>
                          <span className="font-mono text-xs text-muted-foreground">
                            {repository.base_branch}
                          </span>
                        </span>
                        {repository.description ? (
                          <span className="truncate text-xs text-muted-foreground">
                            {repository.description}
                          </span>
                        ) : null}
                      </div>
                      {/* cmdk owns `aria-selected` for the highlight, so being
                          picked has to be said in words of its own. */}
                      {selected ? <span className="sr-only">Selected</span> : null}
                    </CommandItem>
                  )
                })}
              </CommandList>
            </Command>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}
