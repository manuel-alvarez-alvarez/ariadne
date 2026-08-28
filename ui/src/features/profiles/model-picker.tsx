/**
 * A model field: a free-text input with the daemon's model catalog (`GET
 * /v1/models`) hanging under it.
 *
 * Free text is the contract and the catalog is only a convenience, so this is
 * built the opposite way round from a select: the field itself is a cmdk input
 * — whatever is typed *is* the value, matched or not — and the suggestions live
 * in a Base UI popover anchored to it. The popover has no trigger of its own
 * and never takes focus (`initialFocus={false}`): it opens on click, typing or
 * ↓, and the keyboard stays in the input throughout.
 *
 * Two behaviours are deliberately preserved from the plain `Input` this
 * replaces:
 *
 * - Enter submits the form unless the highlight was *actively* moved with the
 *   arrow keys. cmdk auto-highlights the first match while typing, and an Enter
 *   that silently swapped typed free text for that match would make arbitrary
 *   models unsubmittable.
 * - With no catalog to show — loading or errored — the popover simply never
 *   opens and the field is exactly the old free-text input.
 *
 * The value is the whole choice: a qualified id, `<agent_kind>[:<model>]`,
 * which names the agent CLI and then the model of it (see `model-ref.ts`). So
 * there is nothing beside this field to scope it and nothing to wait for
 * before it is worth filling in: the catalog is offered whole, grouped by
 * agent CLI in the order the daemon probes them, each group led by that CLI on
 * its own default model.
 */

import { Popover } from "@base-ui/react/popover"
import { Command as CommandPrimitive } from "cmdk"
import { useMemo, useRef, useState } from "react"

import type { ModelDto } from "@/api"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { inputClassName } from "@/components/ui/input"
import { cn } from "@/lib/format"

import { AGENT_KINDS, agentKindLabel } from "./profile-labels"

export function ModelPicker({
  value,
  onChange,
  models,
  label = "Model",
  placeholder = "Provider default",
  invalid,
  className,
}: {
  value: string
  onChange: (value: string) => void
  /** The catalog, or undefined while it is loading or failed to load. */
  models: ModelDto[] | undefined
  /** The field's accessible name; several pickers on one form each need theirs. */
  label?: string
  placeholder?: string
  invalid?: boolean
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const inputRef = useRef<HTMLInputElement>(null)
  /** True once ↓/↑ moved the highlight, until the text changes again. */
  const navigated = useRef(false)

  /**
   * The catalog under one heading per agent CLI, in the order the daemon
   * probes them, and inside each the CLI on its own default model first: the
   * shortest id of the group is also the one a slot most often wants.
   */
  const groups = useMemo(() => {
    return AGENT_KINDS.map((kind) => ({
      kind,
      models: (models ?? [])
        .filter((model) => model.agent_kind === kind)
        .sort((a, b) => Number(a.id.includes(":")) - Number(b.id.includes(":"))),
    })).filter((group) => group.models.length > 0)
  }, [models])

  const canOpen = groups.length > 0

  function pick(id: string) {
    onChange(id)
    navigated.current = false
    setOpen(false)
  }

  /**
   * Runs before cmdk's own handling, which lives on the Command root the input
   * bubbles into — so `stopPropagation` is how a key is kept native (caret
   * movement, form submission) and `preventDefault` is how cmdk is told a key
   * was already handled.
   */
  function handleKeyDown(event: React.KeyboardEvent<HTMLInputElement>) {
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        if (open) {
          navigated.current = true // cmdk moves the highlight
          return
        }
        event.stopPropagation()
        if (canOpen) {
          event.preventDefault()
          navigated.current = true
          setOpen(true)
        }
        return
      }
      case "Home":
      case "End": {
        // Closed, these move the caret; cmdk would move a hidden highlight.
        if (!open) event.stopPropagation()
        return
      }
      case "Enter": {
        if (open && navigated.current) return // cmdk picks the highlight
        // Exactly the plain input: default form submission, closed list.
        event.stopPropagation()
        setOpen(false)
        return
      }
      case "Escape": {
        if (open) {
          event.preventDefault()
          event.stopPropagation() // the dialog behind must not close with it
          setOpen(false)
        }
        return
      }
    }
  }

  return (
    <div className={cn("flex w-full min-w-0 flex-col gap-1", className)}>
      <Command
        label={label}
        className="size-auto w-full min-w-0 overflow-visible rounded-none bg-transparent"
        onBlur={(event) => {
          // The popup never holds focus, so focus leaving the field closes it.
          if (!event.currentTarget.contains(event.relatedTarget)) setOpen(false)
        }}
      >
        <CommandPrimitive.Input
          ref={inputRef}
          value={value}
          onValueChange={(text) => {
            onChange(text)
            navigated.current = false
            if (!open && canOpen) setOpen(true)
          }}
          onClick={() => {
            if (canOpen) setOpen(true)
          }}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          aria-invalid={invalid ? true : undefined}
          className={cn(inputClassName, "font-mono")}
        />
        <Popover.Root open={open && canOpen} onOpenChange={setOpen} modal={false}>
          <Popover.Portal>
            <Popover.Positioner
              anchor={inputRef}
              side="bottom"
              align="start"
              sideOffset={4}
              className="isolate z-50"
            >
              <Popover.Popup
                initialFocus={false}
                finalFocus={false}
                // Focus must survive a click in the list: the item's own click
                // event only fires if the mousedown before it did not blur the
                // input and close the popup under it.
                onMouseDown={(event) => event.preventDefault()}
                className="w-(--anchor-width) rounded-lg bg-popover text-popover-foreground shadow-md ring-1 ring-foreground/10 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
              >
                <CommandList label="Models" className="max-h-72">
                  <CommandEmpty>No matching models — free text is passed as-is.</CommandEmpty>
                  {groups.map((group) => (
                    <CommandGroup key={group.kind} heading={agentKindLabel(group.kind)}>
                      {group.models.map((model) => (
                        <CommandItem
                          key={model.id}
                          value={model.id}
                          keywords={model.description ? [model.description] : undefined}
                          onSelect={pick}
                        >
                          <div className="flex min-w-0 flex-col">
                            <span className="truncate font-mono text-[13px]">{model.id}</span>
                            {model.description ? (
                              // Two lines, so a long blurb is readable without
                              // one option taking over the list.
                              <span className="line-clamp-2 text-xs leading-snug text-muted-foreground">
                                {model.description}
                              </span>
                            ) : null}
                          </div>
                        </CommandItem>
                      ))}
                    </CommandGroup>
                  ))}
                </CommandList>
              </Popover.Popup>
            </Popover.Positioner>
          </Popover.Portal>
        </Popover.Root>
      </Command>
    </div>
  )
}
