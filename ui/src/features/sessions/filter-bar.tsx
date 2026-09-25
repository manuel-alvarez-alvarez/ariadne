/**
 * The sessions screen's filter bar, piece by piece: the menus the bar opens
 * its single-select filters from, the popovers of the two free-form ones —
 * the directory and the activity window — and the typed field both the search
 * and the directory are.
 *
 * What each filter means, and which half of the listing it reaches, is the
 * screen's (`sessions-page.tsx`); these only draw a filter and report what was
 * picked.
 */

import { useQuery } from "@tanstack/react-query"
import { ChevronDownIcon, XIcon } from "lucide-react"
import { type ComponentProps, useEffect, useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { acpAgentsQueryOptions } from "@/features/agents/queries"
import { cn } from "@/lib/format"

import { ALL } from "./filters"
import { dayBefore, type OutsideFilterParam, type Window } from "./outside-filters"

/**
 * How long a typed filter waits for the next keystroke before the daemon is
 * asked. Both text fields wait: a path is typed as slowly as a search, and
 * neither is worth a request per character.
 */
const TYPING_SETTLES_MS = 250

/** One choice in a {@link FilterMenu}. */
interface FilterOption {
  value: string
  label: string
}

/**
 * One single-select filter of the bar: a compact trigger naming what it
 * filters, and what it is set to once it is set. An unset one is drawn dashed,
 * so what is narrowing the table can be told from what is not at a glance.
 */
export function FilterMenu({
  what,
  value,
  valueLabel,
  onSelect,
  lead = [],
  options,
  allLabel,
}: {
  what: string
  /** The selected option's value, {@link ALL} for none. */
  value: string
  /** What the trigger shows the filter set to, `null` while it is not. */
  valueLabel: string | null
  onSelect: (value: string) => void
  /** Choices above the separator, beside "all": the ones made of several. */
  lead?: FilterOption[]
  options: FilterOption[]
  allLabel: string
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <FilterTrigger
            what={what}
            valueLabel={valueLabel}
            aria-label={`Filter by ${what.toLowerCase()}`}
          />
        }
      />
      <DropdownMenuContent align="start" className="w-44">
        <DropdownMenuRadioGroup value={value} onValueChange={onSelect}>
          <DropdownMenuRadioItem value={ALL}>{allLabel}</DropdownMenuRadioItem>
          {lead.map((option) => (
            <DropdownMenuRadioItem key={option.value} value={option.value}>
              {option.label}
            </DropdownMenuRadioItem>
          ))}
          <DropdownMenuSeparator />
          {options.map((option) => (
            <DropdownMenuRadioItem key={option.value} value={option.value}>
              {option.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * The trigger every filter of the bar opens from: its name, and its value
 * beside it once it has one. Dashed while unset.
 */
function FilterTrigger({
  what,
  valueLabel,
  className,
  ...props
}: ComponentProps<typeof Button> & { what: string; valueLabel: string | null }) {
  return (
    <Button
      variant="outline"
      className={cn(
        "max-w-64 font-normal",
        valueLabel === null && "border-dashed text-muted-foreground",
        className,
      )}
      {...props}
    >
      {what}
      {valueLabel !== null ? (
        <>
          <span aria-hidden className="h-4 w-px bg-border" />
          <span className="min-w-0 truncate font-medium" title={valueLabel}>
            {valueLabel}
          </span>
        </>
      ) : null}
      <ChevronDownIcon className="text-muted-foreground" />
    </Button>
  )
}

/**
 * Which registry agent to show sessions of, out of the ACP registry
 * (`GET /v1/acp-agents`). An agent id is the whole of the choice: it is what
 * tells one ACP agent from another everywhere else on this screen.
 */
export function AgentFilter({
  value,
  onSelect,
}: {
  value: string
  onSelect: (value: string) => void
}) {
  const agents = useQuery(acpAgentsQueryOptions())

  return (
    <FilterMenu
      what="Agent"
      value={value || ALL}
      valueLabel={value || null}
      onSelect={onSelect}
      options={(agents.data ?? []).map((agent) => ({ value: agent.id, label: agent.id }))}
      allLabel="All agents"
    />
  )
}

/** The window's three choices, in the order the trigger offers them. */
const WINDOW_OPTIONS: { value: Window; label: string }[] = [
  { value: "7", label: "Last 7 days" },
  { value: "30", label: "Last 30 days" },
  { value: "all", label: "All time" },
]

/**
 * How far back the table looks: the last 7 days (the daemon's own default),
 * the last 30, or all of it. Unlike the other triggers of the bar this one is
 * never unset — the table always has a window, 7 days being the one it opens
 * on — so it draws solid rather than dashed, and offers no "all ___" choice
 * of its own beside the three.
 */
export function WindowFilter({
  value,
  onSelect,
}: {
  value: Window
  onSelect: (value: string) => void
}) {
  const label = WINDOW_OPTIONS.find((option) => option.value === value)?.label ?? null
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={<FilterTrigger what="Window" valueLabel={label} aria-label="Filter by window" />}
      />
      <DropdownMenuContent align="start" className="w-44">
        <DropdownMenuRadioGroup value={value} onValueChange={onSelect}>
          {WINDOW_OPTIONS.map((option) => (
            <DropdownMenuRadioItem key={option.value} value={option.value}>
              {option.label}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/**
 * Where a session ran: the directory it is in, or one above it. Typed, so
 * it opens as a field in a popover rather than as a menu of choices, and the
 * trigger names only the last part of the path.
 */
export function DirectoryFilter({
  value,
  onSettle,
}: {
  value: string
  onSettle: (param: OutsideFilterParam, value: string) => void
}) {
  return (
    <Popover>
      <PopoverTrigger
        render={
          <FilterTrigger
            what="Directory"
            valueLabel={value ? (value.split("/").filter(Boolean).at(-1) ?? value) : null}
            aria-label="Filter by directory"
          />
        }
      />
      <PopoverContent align="start" className="w-96">
        <Field>
          <FieldLabel htmlFor="sessions-directory">Working directory</FieldLabel>
          <TypedFilter
            id="sessions-directory"
            label="Working directory"
            placeholder="/Users/me/dev"
            value={value}
            onSettle={onSettle}
            param="dir"
            className="font-mono text-xs"
            autoFocus
          />
          <FieldDescription>Sessions that ran in this directory, or below it.</FieldDescription>
        </Field>
      </PopoverContent>
    </Popover>
  )
}

/** The windows most looked for, one click each: `since` so many days back. */
const ACTIVITY_PRESETS = [
  { label: "Today", days: 0 },
  { label: "Last 7 days", days: 6 },
  { label: "Last 30 days", days: 29 },
] as const

const DAY_LABEL = new Intl.DateTimeFormat("en", { day: "numeric", month: "short", timeZone: "UTC" })

/** `2026-09-25` as `25 Sep`; anything else as it stands. */
function dayLabel(day: string): string {
  const at = new Date(`${day}T00:00:00Z`)
  return Number.isNaN(at.getTime()) ? day : DAY_LABEL.format(at)
}

function activityLabel(since: string, until: string): string | null {
  if (since && until)
    return since === until ? dayLabel(since) : `${dayLabel(since)} – ${dayLabel(until)}`
  if (since) {
    const preset = ACTIVITY_PRESETS.find(({ days }) => dayBefore(days) === since)
    return preset ? preset.label : `Since ${dayLabel(since)}`
  }
  if (until) return `Until ${dayLabel(until)}`
  return null
}

/**
 * When a session was last active: a preset window, or a day on either side.
 * Unset, the daemon lists the outside conversations of the last week and every
 * session Ariadne ran, which the popover says, since nothing else would.
 */
export function ActivityFilter({
  since,
  until,
  onChange,
}: {
  since: string
  until: string
  onChange: (values: Partial<Record<OutsideFilterParam, string>>) => void
}) {
  return (
    <Popover>
      <PopoverTrigger
        render={
          <FilterTrigger
            what="Activity"
            valueLabel={activityLabel(since, until)}
            aria-label="Filter by activity"
          />
        }
      />
      <PopoverContent align="start" className="w-80">
        <div className="flex flex-wrap gap-1.5">
          {ACTIVITY_PRESETS.map(({ label, days }) => {
            const day = dayBefore(days)
            return (
              <Button
                key={label}
                variant={since === day && !until ? "secondary" : "outline"}
                size="sm"
                onClick={() => onChange({ since: day, until: "" })}
              >
                {label}
              </Button>
            )
          })}
        </div>
        <div className="grid grid-cols-2 gap-2">
          <Field>
            <FieldLabel htmlFor="sessions-since">From</FieldLabel>
            <DayField
              id="sessions-since"
              label="Active since"
              value={since}
              max={until}
              onChange={(day) => onChange({ since: day })}
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="sessions-until">To</FieldLabel>
            <DayField
              id="sessions-until"
              label="Active until"
              value={until}
              min={since}
              onChange={(day) => onChange({ until: day })}
            />
          </Field>
        </div>
        <p className="text-xs text-muted-foreground">
          Unset, outside conversations of the last 7 days are listed.
        </p>
        {since || until ? (
          <Button
            variant="ghost"
            size="sm"
            className="self-start"
            onClick={() => onChange({ since: "", until: "" })}
          >
            <XIcon />
            Any time
          </Button>
        ) : null}
      </PopoverContent>
    </Popover>
  )
}

/**
 * One day of the activity window. Empty, it is a text field saying so: WebKit
 * draws an empty date field as today's date, which reads as a day that was
 * picked. It turns into the date field once focused or filled.
 */
function DayField({
  id,
  label,
  value,
  min,
  max,
  onChange,
}: {
  id: string
  label: string
  value: string
  min?: string
  max?: string
  onChange: (day: string) => void
}) {
  const [focused, setFocused] = useState(false)
  return (
    <Input
      id={id}
      type={value || focused ? "date" : "text"}
      aria-label={label}
      placeholder="Any day"
      value={value}
      min={min || undefined}
      max={max || undefined}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      onChange={(event) => onChange(event.target.value)}
    />
  )
}

/**
 * A filter that is typed: the field shows every keystroke, and the daemon is
 * asked once the typing has settled (see {@link TYPING_SETTLES_MS}).
 *
 * Typing still unsettled when the field goes — a popover closed mid-word — is
 * sent then rather than dropped, and a value the URL changes underneath it
 * (Clear filters) is what the field shows next, unless it is being typed in.
 */
export function TypedFilter({
  id,
  label,
  placeholder,
  value,
  param,
  onSettle,
  className,
  autoFocus,
}: {
  id?: string
  label: string
  placeholder: string
  /** What the URL carries, which is what the field opens on. */
  value: string
  param: OutsideFilterParam
  onSettle: (param: OutsideFilterParam, value: string) => void
  className?: string
  autoFocus?: boolean
}) {
  const [text, setText] = useState(value)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const unsettled = useRef<string | null>(null)
  const settle = useRef(onSettle)
  settle.current = onSettle

  useEffect(() => {
    if (unsettled.current === null) setText(value)
  }, [value])

  useEffect(
    () => () => {
      clearTimeout(timer.current ?? undefined)
      if (unsettled.current !== null) settle.current(param, unsettled.current)
    },
    [param],
  )

  return (
    <Input
      id={id}
      value={text}
      aria-label={label}
      placeholder={placeholder}
      autoComplete="off"
      autoFocus={autoFocus}
      className={className}
      onChange={(event) => {
        const next = event.target.value
        setText(next)
        unsettled.current = next
        clearTimeout(timer.current ?? undefined)
        timer.current = setTimeout(() => {
          unsettled.current = null
          settle.current(param, next)
        }, TYPING_SETTLES_MS)
      }}
    />
  )
}
