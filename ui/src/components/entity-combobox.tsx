/**
 * One combobox, shared by the Skills and Workflows screens: a trigger that
 * reads as the selected row, and a filterable catalog behind it, grouped by
 * where each row came from.
 *
 * Both screens put every row down a left-hand list beside the editor, the
 * shape the profiles screen once had. The catalog outgrew what a scroll box
 * reads well, so both move to the pattern `PinPicker` and
 * `RepositoryCombobox` already carry for theirs: a Base UI popover over a
 * cmdk list, searched rather than scrolled.
 */

import { Popover } from "@base-ui/react/popover"
import { ChevronsUpDownIcon } from "lucide-react"
import type { ReactNode } from "react"
import { useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { cn } from "@/lib/format"

interface ComboboxEntity {
  name: string
  /** The line shown under the name, in the trigger and in the row alike. */
  details: string
  builtin: boolean
  /** A marker beside the name — the "edited" badge a skill earns, say. */
  badge?: ReactNode
}

export function EntityCombobox({
  id,
  label,
  entityLabel,
  entities,
  selectedName,
  onSelect,
  placeholder,
  className,
}: {
  id?: string
  /** The control's accessible name: the trigger, the search box and the list all share it. */
  label: string
  /** What one row is, lowercase — "skill", "workflow" — named in the empty message. */
  entityLabel: string
  entities: ComboboxEntity[]
  selectedName: string | null
  onSelect: (name: string) => void
  placeholder: string
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState("")
  const searchRef = useRef<HTMLInputElement>(null)
  const selected = entities.find((entity) => entity.name === selectedName)

  const groups = [
    { key: "shipped", title: "Shipped with Ariadne", entities: entities.filter((e) => e.builtin) },
    { key: "yours", title: "Yours", entities: entities.filter((e) => !e.builtin) },
  ]

  return (
    <Popover.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        // Every open starts from the whole catalog, not the last search.
        if (next) setSearch("")
      }}
      modal={false}
    >
      <Popover.Trigger
        id={id}
        render={
          <Button
            type="button"
            variant="outline"
            aria-label={label}
            className={cn("w-full justify-between font-normal sm:w-96", className)}
          />
        }
      >
        <span className="flex min-w-0 flex-1 items-baseline gap-2 text-left">
          {selected ? (
            <>
              <span className="min-w-0 truncate font-medium">{selected.name}</span>
              {selected.badge}
            </>
          ) : (
            <span className="truncate text-muted-foreground">{placeholder}</span>
          )}
        </span>
        <ChevronsUpDownIcon className="shrink-0 opacity-50" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="bottom" align="start" sideOffset={4} className="isolate z-50">
          <Popover.Popup
            initialFocus={searchRef}
            aria-label={label}
            className="flex w-(--anchor-width) min-w-80 flex-col rounded-lg bg-popover text-popover-foreground shadow-md ring-1 ring-foreground/10 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
          >
            <Command label={label}>
              <CommandInput
                ref={searchRef}
                value={search}
                onValueChange={setSearch}
                placeholder={`Filter ${entityLabel}s…`}
              />
              <CommandList
                label={label}
                className="max-h-80"
                // A pick must not take focus off the search box, or the next
                // one would have to reach for it again.
                onMouseDown={(event) => event.preventDefault()}
              >
                <CommandEmpty>
                  No {entityLabel} matches “{search}”.
                </CommandEmpty>
                {groups.map((group) =>
                  group.entities.length > 0 ? (
                    <CommandGroup key={group.key} heading={group.title}>
                      {group.entities.map((entity) => (
                        <CommandItem
                          key={entity.name}
                          value={entity.name}
                          keywords={[entity.details]}
                          data-checked={entity.name === selectedName ? "true" : "false"}
                          onSelect={() => {
                            onSelect(entity.name)
                            setOpen(false)
                          }}
                        >
                          <div className="flex min-w-0 flex-1 flex-col">
                            <span className="flex items-baseline gap-2">
                              <span className="min-w-0 truncate font-medium">{entity.name}</span>
                              {entity.badge}
                            </span>
                            <span className="truncate text-xs text-muted-foreground">
                              {entity.details}
                            </span>
                          </div>
                        </CommandItem>
                      ))}
                    </CommandGroup>
                  ) : null,
                )}
              </CommandList>
            </Command>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}
