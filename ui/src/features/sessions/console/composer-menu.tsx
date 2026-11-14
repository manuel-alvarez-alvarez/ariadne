/**
 * The plus menu beside the composer: the agent's slash commands, searchable,
 * and the actions of the session: Switch agent/model, Resume once it has
 * ended, Expand or collapse all, and Copy loaded transcript.
 *
 * A command is inserted into the composer for editing and is never sent from
 * here. Every entry is a button, so a click, a tap, Enter and Space all pick
 * it, and the arrow keys walk the list from the search field.
 */

import {
  ArrowRightLeftIcon,
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  ClipboardCopyIcon,
  PlayIcon,
  PlusIcon,
} from "lucide-react"
import { type KeyboardEvent, useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"

import type { AgentCommand } from "./transcript"

type Entry = {
  key: string
  label: string
  detail?: string
  icon?: React.ReactNode
  run: () => void
}

export function ComposerMenu({
  open,
  onOpenChange,
  commands,
  onCommand,
  onSwitch,
  onResume,
  resumePending,
  folded,
  onToggleFold,
  copyLabel,
  onCopy,
  returnFocus,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  commands: AgentCommand[]
  onCommand: (name: string) => void
  /** None where the session has no switch to offer. */
  onSwitch?: () => void
  /** None while the session is live. */
  onResume?: () => void
  resumePending: boolean
  folded: boolean
  onToggleFold: () => void
  copyLabel: string
  onCopy: () => void
  /** Where the keyboard goes after a pick. */
  returnFocus: () => HTMLElement | null
}) {
  const [search, setSearch] = useState("")
  const list = useRef<HTMLDivElement>(null)
  const picked = useRef(false)
  const searchField = useRef<HTMLInputElement>(null)

  const actions: Entry[] = [
    ...(onSwitch
      ? [
          {
            key: "switch",
            label: "Switch agent/model",
            icon: <ArrowRightLeftIcon />,
            run: onSwitch,
          },
        ]
      : []),
    ...(onResume && !resumePending
      ? [{ key: "resume", label: "Resume", icon: <PlayIcon />, run: onResume }]
      : []),
    {
      key: "fold",
      label: folded ? "Expand all" : "Collapse all",
      icon: folded ? <ChevronsUpDownIcon /> : <ChevronsDownUpIcon />,
      run: onToggleFold,
    },
    { key: "copy", label: copyLabel, icon: <ClipboardCopyIcon />, run: onCopy },
  ]
  const entries: Entry[] = [
    ...commands.map((command) => ({
      key: `command:${command.name}`,
      label: `/${command.name}`,
      detail: command.description,
      run: () => onCommand(command.name),
    })),
    ...actions,
  ]
  const needle = search.trim().toLowerCase()
  const shown = entries.filter((entry) =>
    `${entry.label} ${entry.detail ?? ""}`.toLowerCase().includes(needle),
  )

  function pick(entry: Entry) {
    picked.current = entry.key !== "switch"
    onOpenChange(false)
    entry.run()
  }

  function items(): HTMLElement[] {
    return Array.from(list.current?.querySelectorAll<HTMLElement>("button") ?? [])
  }

  function onSearchKey(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault()
      items()[0]?.focus()
    } else if (event.key === "Enter" && shown[0]) {
      event.preventDefault()
      pick(shown[0])
    }
  }

  function onItemKey(event: KeyboardEvent) {
    const all = items()
    const at = all.indexOf(document.activeElement as HTMLElement)
    if (event.key === "ArrowDown") all[Math.min(at + 1, all.length - 1)]?.focus()
    else if (event.key === "ArrowUp") {
      if (at <= 0) (list.current?.previousElementSibling as HTMLElement | null)?.focus()
      else all[at - 1]?.focus()
    } else return
    event.preventDefault()
  }

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (next) {
          setSearch("")
          picked.current = false
        }
        onOpenChange(next)
      }}
    >
      <PopoverTrigger
        render={
          <Button
            size="icon-sm"
            variant="outline"
            aria-label="Commands and session actions"
            type="button"
          />
        }
      >
        <PlusIcon />
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        className="w-80 gap-1.5"
        initialFocus={searchField}
        finalFocus={() => (picked.current ? returnFocus() : true)}
      >
        <Input
          ref={searchField}
          aria-label="Search commands and actions"
          placeholder="Search commands and actions"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
          onKeyDown={onSearchKey}
        />
        <div ref={list} className="max-h-64 overflow-y-auto">
          {shown.length === 0 ? (
            <p className="px-2 py-1.5 text-xs text-muted-foreground">Nothing matches.</p>
          ) : (
            shown.map((entry) => (
              <button
                key={entry.key}
                type="button"
                onClick={() => pick(entry)}
                onKeyDown={onItemKey}
                className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm outline-none hover:bg-muted focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 [&_svg]:size-4 [&_svg]:shrink-0"
              >
                {entry.icon}
                <span className={entry.detail === undefined ? "min-w-0" : "shrink-0 font-mono"}>
                  {entry.label}
                </span>
                {entry.detail ? (
                  <span className="min-w-0 truncate text-xs text-muted-foreground">
                    {entry.detail}
                  </span>
                ) : null}
              </button>
            ))
          )}
        </div>
      </PopoverContent>
    </Popover>
  )
}
