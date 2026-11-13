/**
 * What an agent runs on, as one control: the model, and the effort that model
 * is run at.
 *
 * The trigger reads like a sentence — the agent's icon, the model, the effort
 * — and everything that changes it lives in one popover of three columns, in
 * the order the choice is made: the coding agent, then one model of it, then
 * the effort that model can be run at. A search box above them answers across
 * every agent at once, as one flat list in place of the first two columns.
 *
 * Neither half changes what goes on the wire. The value is still two strings,
 * held by the form:
 *
 * - the model, `<agent>:<model>` — the registry id of an agent and, after the
 *   first `:`, the model of it (see `model-ref.ts`). The catalog is the only
 *   way to choose one here; a pin saved before its model left the catalog
 *   still reads on the trigger as it is;
 * - the effort, which is the *model's* closed list as discovery found it: one
 *   model takes five levels, another takes none at all, and a model discovery
 *   has not seen takes whichever effort its agent alone was configured with,
 *   so nothing here can name them and the column becomes a text box.
 *
 * A model is required. No effort is the model's default effort, which the
 * column offers as `auto (high)` rather than as a blank.
 *
 * A model turned off on the agents screen remains a manual choice, so the
 * columns list it once "Show disabled" is on — muted, and marked off — and a
 * search always does. A turned-off pin turns the switch on, when the popover
 * opens on it and when a search picks it. The switch is one setting for every
 * picker, kept with the app's own.
 *
 * The rules the daemon enforces (`ariadne_core::models::effort_error`,
 * `http/pins.rs`) are applied here rather than after a round trip: an effort
 * is dropped when the model moves out from under it.
 */

import { Popover } from "@base-ui/react/popover"
import { useQuery } from "@tanstack/react-query"
import { ChevronsUpDownIcon } from "lucide-react"
import {
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react"

import type { EffortDto, ModelDto } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { AgentIcon } from "@/features/agents/agent-icon"
import { acpAgentsQueryOptions } from "@/features/agents/queries"
import { cn } from "@/lib/format"
import { useSettingsStore } from "@/stores/settings"

import { parseModelRef } from "./model-ref"

/** The whole choice, which is what the forms hold and what a pick yields. */
interface Pin {
  /** `<agent>:<model>`. */
  model: string
  /** One of that model's efforts, or the empty string for the agent's own. */
  effort: string
}

/** What an unpinned effort is called, in the column and on the trigger. */
const AUTO_EFFORT_LABEL = "auto"

export function PinPicker({
  model,
  effort,
  onChange,
  models,
  label,
  invalid,
  id,
  className,
}: {
  /** The chosen model, or the empty string until one is chosen. */
  model: string
  /** The pinned effort, or the empty string for the agent's own. */
  effort: string
  onChange: (pin: Pin) => void
  /** The catalog, or undefined while it is loading or failed to load. */
  models: ModelDto[] | undefined
  /** The control's accessible name; several pickers on one form each need theirs. */
  label: string
  invalid?: boolean
  /** The trigger's id, for a field label's `for`. */
  id?: string
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState("")
  /** The agent the user browsed to, or empty for the pinned one. */
  const [browsed, setBrowsed] = useState("")
  /** The column a pick hands the keyboard to, once it has rendered. */
  const [focusRequest, setFocusRequest] = useState<{ column: "models" | "efforts" } | null>(null)
  const showDisabled = useSettingsStore((state) => state.showDisabledModels)
  const setShowDisabled = useSettingsStore((state) => state.setShowDisabledModels)
  const searchRef = useRef<HTMLInputElement>(null)
  const agentsRef = useRef<HTMLDivElement>(null)
  const modelsRef = useRef<HTMLDivElement>(null)
  const effortsRef = useRef<HTMLDivElement>(null)

  // The registry's icon URLs; without them the bundled icons and the letters
  // still draw, so a failed request needs nothing said about it here.
  const acpAgents = useQuery(acpAgentsQueryOptions())
  const icons = useMemo(
    () => new Map(acpAgents.data?.map((agent) => [agent.id, agent.icon])),
    [acpAgents.data],
  )

  const pinned = model.trim()
  const pinnedAgent = parseModelRef(pinned)?.agent ?? ""

  // The switch alone decides whether a turned-off model is listed, the pin's
  // own included: a turned-off pin turns the switch on instead (on open, and
  // on a pick from a search), so the columns can still show it selected.
  const shown = useMemo(
    () => (models ?? []).filter((entry) => entry.enabled || showDisabled),
    [models, showDisabled],
  )
  const pinnedEntry = models?.find((entry) => entry.id === pinned)
  /** A pin the catalog lists but the switch keeps out of the columns. */
  const pinHidden = pinnedEntry !== undefined && !pinnedEntry.enabled && !showDisabled
  const agents = useMemo(() => [...new Set(shown.map((entry) => entry.agent_id))], [shown])
  // The agent browsed to, else the pin's, else the first: a pin whose agent
  // has nothing to show opens on a column that does.
  const agent =
    browsed || (agents.includes(pinnedAgent) || agents.length === 0 ? pinnedAgent : agents[0]) || ""
  const agentModels = shown.filter((entry) => entry.agent_id === agent)
  /** The model the effort column answers for: the pin, while it shows under the agent browsed. */
  const chosen = pinnedAgent === agent && !pinHidden ? pinned : ""
  const choices = useMemo(() => effortChoices(chosen, models), [chosen, models])
  const pinChoices = useMemo(() => effortChoices(pinned, models), [pinned, models])

  const query = search.trim().toLowerCase()
  const results = query.length > 0 ? (models ?? []).filter((entry) => matches(entry, query)) : []

  // An effort belongs to the model it runs at, so a model moved out from under
  // one that does not take it clears the effort — the daemon's own rule
  // for a repin whose effort does not travel with it, applied where it can
  // still be seen rather than refused on submit. The model can move from
  // outside too (a reviewer row's profile changed under an empty pin), which
  // is why this is an effect and not only part of a pick.
  useEffect(() => {
    if (effort.length === 0) return
    if (
      pinChoices.kind === "none" ||
      (pinChoices.kind === "options" && !pinChoices.efforts.some((one) => one.id === effort))
    )
      onChange({ model, effort: "" })
  }, [pinChoices, effort, model, onChange])

  // Escape answers the open popover and stops there: this picker is always
  // inside a form dialog, and that dialog closing with it would throw away
  // everything typed into the form. It is taken at the document in the capture
  // phase rather than on the popup itself because the key can be pressed with
  // focus anywhere — a pick that empties a column takes the focused row away
  // with it, leaving the body focused.
  useEffect(() => {
    if (!open) return
    function dismiss(event: globalThis.KeyboardEvent) {
      if (event.key !== "Escape") return
      event.preventDefault()
      event.stopPropagation()
      event.stopImmediatePropagation()
      setOpen(false)
    }
    document.addEventListener("keydown", dismiss, true)
    return () => document.removeEventListener("keydown", dismiss, true)
  }, [open])

  // A pick hands the keyboard on to the next column, once that column holds
  // what the pick put in it. A column with nothing to focus — a model that
  // takes no effort — leaves it on the model just picked.
  useEffect(() => {
    if (!focusRequest) return
    const order = focusRequest.column === "efforts" ? [effortsRef, modelsRef] : [modelsRef]
    for (const ref of order) if (focusColumn(ref)) return
  }, [focusRequest])

  function pickAgent(next: string) {
    setBrowsed(next)
    setFocusRequest({ column: "models" })
  }

  function pickModel(entry: ModelDto) {
    const moved = effortChoices(entry.id, models)
    const travels =
      effort.length > 0 &&
      moved.kind === "options" &&
      moved.efforts.some((one) => one.id === effort)
    setSearch("")
    setBrowsed(entry.agent_id)
    if (!entry.enabled) setShowDisabled(true)
    onChange({ model: entry.id, effort: travels ? effort : "" })
    setFocusRequest({ column: "efforts" })
  }

  function pickEffort(next: string) {
    onChange({ model: pinned, effort: next })
    setOpen(false)
  }

  function onSearchKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault()
      focusColumn(query.length > 0 ? modelsRef : agentsRef)
    } else if (event.key === "Enter") {
      event.preventDefault()
      const first = results[0]
      if (first) pickModel(first)
    }
  }

  function modelRow(entry: ModelDto, withIcon: boolean): OptionRow {
    return {
      key: entry.id,
      selected: entry.id === pinned,
      muted: !entry.enabled,
      onPick: () => pickModel(entry),
      content: (
        <>
          {withIcon ? (
            <AgentIcon
              agentId={entry.agent_id}
              icon={icons.get(entry.agent_id)}
              className="mt-0.5"
            />
          ) : null}
          <span className="flex min-w-0 flex-1 flex-col">
            <span data-slot="option-label" className="truncate font-mono text-[13px]">
              {entry.id}
            </span>
            {entry.description ? (
              // Two lines, so a long blurb is readable without
              // one option taking over the list.
              <span className="line-clamp-2 text-xs leading-snug text-muted-foreground">
                {entry.description}
              </span>
            ) : null}
          </span>
          {entry.enabled ? null : (
            <Badge variant="outline" className="h-4 px-1.5 text-[10px] text-muted-foreground">
              off
            </Badge>
          )}
        </>
      ),
    }
  }

  const agentRows: OptionRow[] = agents.map((one) => ({
    key: one,
    selected: one === agent,
    onPick: () => pickAgent(one),
    content: (
      <>
        <AgentIcon agentId={one} icon={icons.get(one)} />
        <span data-slot="option-label" className="truncate">
          {one}
        </span>
      </>
    ),
  }))

  return (
    <Popover.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        // Every open starts from the columns, on whatever is pinned.
        if (next) {
          setSearch("")
          setBrowsed("")
          if (pinnedEntry && !pinnedEntry.enabled) setShowDisabled(true)
        }
      }}
      modal={false}
    >
      <Popover.Trigger
        render={
          <Button
            id={id}
            type="button"
            variant="outline"
            aria-label={label}
            aria-invalid={invalid ? true : undefined}
            className={cn("w-full justify-between font-normal", className)}
          />
        }
      >
        <span className="flex min-w-0 flex-1 items-center gap-1.5 text-left">
          <TriggerLabel model={pinned} effort={effort} icons={icons} />
        </span>
        <ChevronsUpDownIcon className="shrink-0 opacity-50" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="bottom" align="start" sideOffset={4} className="isolate z-50">
          <Popover.Popup
            initialFocus={searchRef}
            aria-label={label}
            className="flex w-[44rem] max-w-[calc(100vw-2rem)] flex-col rounded-lg bg-popover text-popover-foreground shadow-md ring-1 ring-foreground/10 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
          >
            <div className="flex items-center gap-3 border-b p-2">
              <Input
                ref={searchRef}
                type="search"
                aria-label="Search models"
                placeholder="Search models…"
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                onKeyDown={onSearchKeyDown}
                autoComplete="off"
                spellCheck={false}
              />
              {/* biome-ignore lint/a11y/noLabelWithoutControl: the switch inside is the control */}
              <label className="flex shrink-0 cursor-pointer items-center gap-2 text-xs text-muted-foreground select-none">
                <Switch size="sm" checked={showDisabled} onCheckedChange={setShowDisabled} />
                Show disabled
              </label>
            </div>
            <div className="grid grid-cols-[11rem_minmax(0,1fr)_11rem] divide-x">
              {query.length > 0 ? (
                <Column
                  heading="Model"
                  columnRef={modelsRef}
                  className="col-span-2"
                  empty={results.length === 0 ? "Nothing in the catalog matches." : undefined}
                >
                  <OptionList
                    label="Models"
                    rows={results.map((entry) => modelRow(entry, true))}
                    onTop={() => searchRef.current?.focus()}
                  />
                </Column>
              ) : (
                <>
                  <Column
                    heading="Agent"
                    columnRef={agentsRef}
                    empty={
                      agents.length > 0
                        ? undefined
                        : models
                          ? "No agent has a model to show."
                          : "Loading the catalog…"
                    }
                  >
                    <OptionList label="Agents" rows={agentRows} onRight />
                  </Column>
                  <Column
                    heading="Model"
                    columnRef={modelsRef}
                    empty={
                      agentModels.length > 0
                        ? undefined
                        : agent
                          ? `${agent} has no model to show.`
                          : "Choose an agent."
                    }
                  >
                    <OptionList
                      label="Models"
                      rows={agentModels.map((entry) => modelRow(entry, false))}
                      onLeft={() => focusColumn(agentsRef)}
                      onRight
                    />
                  </Column>
                </>
              )}
              <Column heading="Effort" columnRef={effortsRef}>
                <EffortColumn
                  choices={choices}
                  value={effort}
                  onPick={pickEffort}
                  onType={(next) => onChange({ model: pinned, effort: next })}
                  onDone={() => setOpen(false)}
                  onLeft={() => focusColumn(modelsRef)}
                />
              </Column>
            </div>
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}

/**
 * The pin as the trigger reads it: a sentence rather than two boxes.
 *
 * The agent is its icon — it is also the column the model was picked under —
 * and the model is the part that identifies the choice, so that is what
 * survives the truncation of a narrow row.
 */
function TriggerLabel({
  model,
  effort,
  icons,
}: {
  model: string
  effort: string
  icons: Map<string, string | null | undefined>
}) {
  if (model.length === 0) return <span className="text-muted-foreground">Choose a model</span>
  const ref = parseModelRef(model)
  // Text the daemon will judge: shown as typed, since nothing here can split
  // an id it does not recognise.
  if (!ref) return <span className="truncate font-mono">{model}</span>
  return (
    <>
      <AgentIcon agentId={ref.agent} icon={icons.get(ref.agent)} />
      <span className="truncate">
        <span className="font-mono">{ref.model}</span>
        {effort.length > 0 ? (
          <>
            <span className="text-muted-foreground"> · </span>
            <span className="font-mono">{effort}</span>
          </>
        ) : null}
      </span>
    </>
  )
}

/** Whether a search matches a model: every word, in its id or in what its agent said of it. */
function matches(entry: ModelDto, query: string): boolean {
  const text = `${entry.id} ${entry.description ?? ""}`.toLowerCase()
  return query.split(/\s+/).every((term) => text.includes(term))
}

/** Hands the keyboard to a column's current row, or to its text box; false where it has neither. */
function focusColumn(ref: RefObject<HTMLDivElement | null>): boolean {
  const target = ref.current?.querySelector<HTMLElement>('[role="option"][tabindex="0"], input')
  target?.focus()
  return Boolean(target)
}

/** One column of the popover: a caption, its list, and what it says when the list is empty. */
function Column({
  heading,
  columnRef,
  className,
  empty,
  children,
}: {
  heading: string
  columnRef: RefObject<HTMLDivElement | null>
  className?: string
  /** What an empty list says, given only where the list is empty. */
  empty?: string
  children: ReactNode
}) {
  return (
    <div
      ref={columnRef}
      className={cn("flex max-h-72 min-h-40 min-w-0 flex-col overflow-y-auto p-1", className)}
    >
      <p className="px-2 pt-1 pb-1.5 text-[11px] font-medium text-muted-foreground uppercase">
        {heading}
      </p>
      {children}
      {empty ? <p className="px-2 py-1 text-xs text-muted-foreground">{empty}</p> : null}
    </div>
  )
}

interface OptionRow {
  key: string
  selected: boolean
  /** A turned-off model: listed, and muted. */
  muted?: boolean
  onPick: () => void
  content: ReactNode
}

/**
 * One column's rows, as a listbox the keyboard walks: Up and Down within it,
 * Enter or Space to pick, Right to pick and move on, Left back to the column
 * before. Only the selected row — or the first — is in the tab order, so Tab
 * moves from column to column.
 */
function OptionList({
  label,
  rows,
  onLeft,
  onRight,
  onTop,
}: {
  label: string
  rows: OptionRow[]
  onLeft?: () => void
  /** Whether Right picks the row, as it does in every column but the last. */
  onRight?: boolean
  /** Where Up goes from the first row. */
  onTop?: () => void
}) {
  const active = Math.max(
    0,
    rows.findIndex((row) => row.selected),
  )

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>, index: number) {
    const options = [
      ...(event.currentTarget.parentElement?.querySelectorAll<HTMLElement>('[role="option"]') ??
        []),
    ]
    switch (event.key) {
      case "ArrowDown":
        options[Math.min(index + 1, options.length - 1)]?.focus()
        break
      case "ArrowUp":
        if (index === 0 && onTop) onTop()
        else options[Math.max(index - 1, 0)]?.focus()
        break
      case "Home":
        options[0]?.focus()
        break
      case "End":
        options.at(-1)?.focus()
        break
      case "Enter":
      case " ":
        rows[index]?.onPick()
        break
      case "ArrowRight":
        if (!onRight) return
        rows[index]?.onPick()
        break
      case "ArrowLeft":
        if (!onLeft) return
        onLeft()
        break
      default:
        return
    }
    event.preventDefault()
  }

  return (
    <div role="listbox" aria-label={label} className="flex flex-col gap-0.5">
      {rows.map((row, index) => (
        <div
          key={row.key}
          role="option"
          aria-selected={row.selected}
          tabIndex={index === active ? 0 : -1}
          onClick={row.onPick}
          onKeyDown={(event) => onKeyDown(event, index)}
          className={cn(
            "group flex cursor-pointer items-start gap-2 rounded-md px-2 py-1.5 text-sm outline-none select-none hover:bg-muted focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-ring/50 aria-selected:bg-accent aria-selected:text-accent-foreground",
            row.muted && "text-muted-foreground",
          )}
        >
          {row.content}
        </div>
      ))}
    </div>
  )
}

/**
 * The efforts the chosen model can be run at, beside the model that chose
 * them. A pick here is the last step of the choice, so it closes the popover.
 */
function EffortColumn({
  choices,
  value,
  onPick,
  onType,
  onDone,
  onLeft,
}: {
  choices: EffortChoices
  value: string
  onPick: (effort: string) => void
  onType: (effort: string) => void
  onDone: () => void
  onLeft: () => void
}) {
  if (choices.kind === "free") {
    return (
      <Input
        aria-label="Effort"
        value={value}
        onChange={(event) => onType(event.target.value)}
        onKeyDown={(event) => {
          if (event.key !== "Enter") return
          event.preventDefault()
          onDone()
        }}
        placeholder={AUTO_EFFORT_LABEL}
        autoComplete="off"
        spellCheck={false}
        className="h-7 font-mono"
      />
    )
  }

  if (choices.kind === "none") {
    return (
      <>
        <OptionList label="Efforts" rows={[]} />
        <p className="px-2 py-1 text-xs text-muted-foreground">{choices.reason}</p>
      </>
    )
  }

  const items: { label: string; description: string | null; value: string }[] = [
    {
      // A choice, not a blank: the agent's own level, named where the catalog
      // says what it is.
      label: choices.defaultEffort
        ? `${AUTO_EFFORT_LABEL} (${choices.defaultEffort})`
        : AUTO_EFFORT_LABEL,
      description: null,
      value: "",
    },
    ...choices.efforts.map((one) => ({
      label: one.id,
      description: one.description ?? null,
      value: one.id,
    })),
  ]

  return (
    <OptionList
      label="Efforts"
      onLeft={onLeft}
      rows={items.map((item) => ({
        key: item.value || AUTO_EFFORT_LABEL,
        selected: value === item.value,
        onPick: () => onPick(item.value),
        content: (
          <span className="flex min-w-0 flex-col">
            <span data-slot="option-label" className="truncate text-xs">
              {item.label}
            </span>
            {item.description ? (
              // Muted under the label, the same way a model's blurb sits
              // under its id.
              <span className="line-clamp-2 text-[11px] leading-snug text-muted-foreground">
                {item.description}
              </span>
            ) : null}
          </span>
        ),
      }))}
    />
  )
}

/** What can be offered beside a model, and why nothing can where nothing is. */
type EffortChoices =
  /** The efforts to offer, cheapest first, and what the agent runs the model at. */
  | { kind: "options"; efforts: EffortDto[]; defaultEffort: string | null }
  /** Nothing knows the list: whatever is typed is passed on, as a model is. */
  | { kind: "free" }
  /** Nothing to choose, and the reason, which the column shows as its hint. */
  | { kind: "none"; reason: string }

/**
 * What the chosen model can be run at, as the catalog says.
 *
 * `model` is the chosen model because that is the model the daemon checks the
 * effort against.
 */
function effortChoices(model: string, models: ModelDto[] | undefined): EffortChoices {
  // What is pinned is answered first, and without the catalog: that there is
  // no model to run an effort at is a fact about the pin, so the column says
  // so while the catalog is still on its way rather than offering a free-text
  // box for an effort the daemon would refuse.
  const id = model.trim()
  if (id.length === 0) {
    return { kind: "none", reason: "An effort is run at a model — choose one first." }
  }
  if (!parseModelRef(id)) {
    return { kind: "none", reason: "An effort is run at a model — that one names no agent." }
  }

  // No catalog — loading, or the endpoint failed — so nothing here can name a
  // list; the column falls back to what the model itself is, free text.
  if (!models) return { kind: "free" }

  const entry = models.find((candidate) => candidate.id === id)
  // A model the catalog knows is held to its own efforts, empty ones included:
  // that is the model saying it takes none at all.
  if (entry) {
    if (entry.efforts.length === 0) {
      return { kind: "none", reason: `${id} takes no effort at all.` }
    }
    const defaultEffort = entry.efforts.find((effort) => effort.default)?.id ?? null
    return { kind: "options", efforts: entry.efforts, defaultEffort }
  }

  // A model the catalog does not carry takes whichever effort its agent was
  // configured with, which only that agent knows: there is no list to hold it
  // to, and whatever is typed is passed on — exactly what the daemon does with
  // one (`effort_error` takes any effort that is not blank).
  return { kind: "free" }
}
