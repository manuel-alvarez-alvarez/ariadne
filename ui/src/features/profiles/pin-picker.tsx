/**
 * What an agent runs on, as one control: the model, and the effort that model
 * is run at.
 *
 * The two are one choice and were two boxes — a free-text model field with the
 * catalog under it, and a select beside it scoped by whatever that field held
 * — which is what made a reviewer row four controls wide and the effort easy
 * to miss entirely. Here the field is a button that reads like a sentence
 * (`Claude Code claude-sonnet-5 · medium`) and everything that changes it
 * lives in one popover: the catalog above, and under it the efforts *that*
 * model can be run at.
 *
 * Neither half changes what goes on the wire. The value is still two strings,
 * held by the form:
 *
 * - the model, `<agent_kind>[:<model>]` — the agent CLI and, after the first
 *   `:`, the model of it (see `model-ref.ts`) — free text the daemon hands to
 *   the CLI as typed, which is why the catalog only suggests and the "Other…"
 *   row at the end of the list is a first-class way to answer;
 * - the effort, which is the *model's* closed list rather than the CLI's:
 *   `claude_code:claude-opus-5` takes five levels, `claude_code:claude-haiku-4-5`
 *   takes none at all, and an opencode model discovery has not seen takes
 *   whichever variants it alone was configured with, so nothing here can name
 *   them and the strip becomes a text box.
 *
 * Empty means something in both, and the two empties are not the same: no
 * model is this slot on its profile's own (or, in the profile form itself,
 * auto — the first installed CLI), which the trigger says in words where the
 * form knows what that resolves to; no effort is the agent CLI's own, which
 * the strip offers as `auto (high)` rather than as a blank.
 *
 * The two are pinned separately because the daemon takes them separately: an
 * effort with no model beside it runs the model the slot would have run on
 * anyway, at that effort (`http/pins.rs`, `chosen`), which is why the strip is
 * scoped by the *effective* model — the pin where there is one, the fallback
 * where there is not — and why the trigger says `Profile's own — <model> ·
 * <effort>` rather than pretending nothing was chosen. Nothing to fall back on
 * is the one case where an effort has no model to run at, and there the strip
 * says so instead of offering one.
 *
 * The rules the daemon enforces (`ariadne_core::models::effort_error`,
 * `http/pins.rs`) are applied here rather than after a round trip: an effort
 * is dropped when the model moves out from under it, and handing the model
 * back to the profile hands its effort back too.
 */

import { Popover } from "@base-ui/react/popover"
import { ChevronsUpDownIcon } from "lucide-react"
import { type ReactNode, useEffect, useId, useMemo, useRef, useState } from "react"

import type { EffortDto, ModelDto } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { Input } from "@/components/ui/input"
import { cn } from "@/lib/format"

import { modelRefError, modelRefLabel, parseModelRef, pinLabel } from "./model-ref"
import { AGENT_KINDS, agentKindLabel } from "./profile-labels"

/** The whole choice, which is what the forms hold and what a pick yields. */
interface Pin {
  /** `<agent_kind>[:<model>]`, or the empty string for the profile's own. */
  model: string
  /** One of that model's efforts, or the empty string for the CLI's own. */
  effort: string
}

/** What an unpinned effort is called, in the strip and on the trigger. */
const AUTO_EFFORT_LABEL = "auto"

/** Shown for an agent CLI pinned with no model after it. */
const DEFAULT_MODEL_LABEL = "default model"

/**
 * cmdk keys its items by value and refuses an empty one, so the two rows that
 * are not catalog entries need names of their own. Both are force-mounted —
 * handing the pin back and typing one the catalog does not carry are answers
 * to every search, not only to a search that happens to match their words.
 */
const UNPINNED_ROW = "__unpinned__"
const OTHER_ROW = "__other__"

export function PinPicker({
  model,
  effort,
  onChange,
  models,
  fallback = null,
  label,
  unpinnedLabel = AUTO_EFFORT_LABEL,
  invalid,
  id,
  className,
}: {
  /** The pinned model, or the empty string for the profile's own. */
  model: string
  /** The pinned effort, or the empty string for the agent CLI's own. */
  effort: string
  onChange: (pin: Pin) => void
  /** The catalog, or undefined while it is loading or failed to load. */
  models: ModelDto[] | undefined
  /**
   * What an empty pin resolves to, where the form knows it: the profile this
   * slot is filled from. Null where nothing can say — the profile form's own
   * field, whose empty is auto and nothing else.
   */
  fallback?: { model: string | null; effort: string | null } | null
  /** The control's accessible name; several pickers on one form each need theirs. */
  label: string
  /** What an empty pin is called where there is no fallback to name. */
  unpinnedLabel?: string
  invalid?: boolean
  /** The trigger's id, for a field label's `for`. */
  id?: string
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState("")
  /** cmdk's highlight, so an open list starts on what is already pinned. */
  const [highlight, setHighlight] = useState(UNPINNED_ROW)
  const searchRef = useRef<HTMLInputElement>(null)
  /** Groups the effort radios, which sit in a portal outside the form. */
  const effortName = useId()

  const pinned = model.trim()
  /**
   * The model this slot will actually run on, which is the one an effort is
   * offered against: the pin where there is one, and what the empty pin
   * resolves to where there is not.
   */
  const running = pinned.length > 0 ? pinned : (fallback?.model ?? "")
  const choices = useMemo(() => effortChoices(running, models), [running, models])

  /**
   * The catalog under one heading per agent CLI, in the order the daemon
   * probes them, and inside each the CLI on its own default model first: the
   * shortest id of the group is also the one a slot most often wants.
   */
  const groups = useMemo(
    () =>
      AGENT_KINDS.map((kind) => ({
        kind,
        models: (models ?? [])
          .filter((entry) => entry.agent_kind === kind)
          .sort((a, b) => Number(a.id.includes(":")) - Number(b.id.includes(":"))),
      })).filter((group) => group.models.length > 0),
    [models],
  )

  // An effort belongs to the model it runs at, so a model moved out from under
  // one that does not take it leaves the pin at auto — the daemon's own rule
  // for a repin whose effort does not travel with it, applied where it can
  // still be seen rather than refused on submit. The model can move from
  // outside too (a reviewer row's profile changed under an empty pin), which
  // is why this is an effect and not only part of a pick.
  useEffect(() => {
    if (effort.length === 0) return
    if (
      choices.kind === "none" ||
      (choices.kind === "options" && !choices.efforts.some((one) => one.id === effort))
    )
      onChange({ model, effort: "" })
  }, [choices, effort, model, onChange])

  // Escape answers the open popover and stops there: this picker is always
  // inside a form dialog, and that dialog closing with it would throw away
  // everything typed into the form. It is taken at the document in the capture
  // phase rather than on the popup itself because the key can be pressed with
  // focus anywhere — a strip whose model no longer takes an effort takes the
  // focused radio away with it, leaving the body focused.
  useEffect(() => {
    if (!open) return
    function dismiss(event: KeyboardEvent) {
      if (event.key !== "Escape") return
      event.preventDefault()
      event.stopPropagation()
      event.stopImmediatePropagation()
      setOpen(false)
    }
    document.addEventListener("keydown", dismiss, true)
    return () => document.removeEventListener("keydown", dismiss, true)
  }, [open])

  function pick(next: string) {
    const trimmed = next.trim()
    // The profile's model comes with the profile's effort: an effort beside a
    // model handed back is refused outright.
    if (trimmed.length === 0) {
      onChange({ model: "", effort: "" })
      return
    }
    const moved = effortChoices(trimmed, models)
    const travels =
      effort.length > 0 &&
      (moved.kind === "free" ||
        (moved.kind === "options" && moved.efforts.some((one) => one.id === effort)))
    onChange({ model: trimmed, effort: travels ? effort : "" })
  }

  /** What the "Other…" row would pin, and why the daemon would refuse it. */
  const typed = search.trim()
  const typedError = modelRefError(typed)

  return (
    <Popover.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        // Every open starts from the whole catalog, on whatever is pinned.
        if (next) {
          setSearch("")
          setHighlight(pinned.length > 0 ? pinned : UNPINNED_ROW)
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
        <span className="min-w-0 flex-1 truncate text-left">
          <TriggerLabel
            model={pinned}
            effort={effort}
            fallback={fallback}
            unpinnedLabel={unpinnedLabel}
          />
        </span>
        <ChevronsUpDownIcon className="shrink-0 opacity-50" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="bottom" align="start" sideOffset={4} className="isolate z-50">
          <Popover.Popup
            initialFocus={searchRef}
            aria-label={label}
            className="flex w-(--anchor-width) min-w-72 flex-col rounded-lg bg-popover text-popover-foreground shadow-md ring-1 ring-foreground/10 data-open:animate-in data-open:fade-in-0 data-closed:animate-out data-closed:fade-out-0"
          >
            <Command label={label} value={highlight} onValueChange={setHighlight}>
              <CommandInput
                ref={searchRef}
                value={search}
                onValueChange={setSearch}
                placeholder="Search models, or type an id…"
              />
              <CommandList
                label="Models"
                className="max-h-64"
                // A pick must not take focus off the search box, or the next
                // one would have to reach for it again.
                onMouseDown={(event) => event.preventDefault()}
              >
                <CommandEmpty>Nothing in the catalog matches.</CommandEmpty>
                <CommandGroup>
                  <CommandItem
                    forceMount
                    value={UNPINNED_ROW}
                    data-checked={pinned.length === 0 ? "true" : "false"}
                    onSelect={() => pick("")}
                  >
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate">{fallback ? "Profile's own" : unpinnedLabel}</span>
                      {fallback ? (
                        <span className="truncate font-mono text-xs text-muted-foreground">
                          {pinLabel(fallback.model, fallback.effort)}
                        </span>
                      ) : null}
                    </span>
                  </CommandItem>
                </CommandGroup>
                {groups.map((group) => (
                  <CommandGroup key={group.kind} heading={agentKindLabel(group.kind)}>
                    {group.models.map((entry) => (
                      <CommandItem
                        key={entry.id}
                        value={entry.id}
                        keywords={modelKeywords(entry)}
                        data-checked={entry.id === pinned ? "true" : "false"}
                        onSelect={pick}
                      >
                        <span className="flex min-w-0 flex-col" title={modelTitle(entry)}>
                          <span className="flex min-w-0 items-center gap-1.5">
                            <span className="truncate font-mono text-[13px]">{entry.id}</span>
                            <ModelPills entry={entry} />
                          </span>
                          {entry.description ? (
                            // Two lines, so a long blurb is readable without
                            // one option taking over the list.
                            <span className="line-clamp-2 text-xs leading-snug text-muted-foreground">
                              {entry.description}
                            </span>
                          ) : null}
                        </span>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                ))}
                <CommandGroup>
                  {/* The daemon takes models the catalog does not list, so
                      whatever was typed has to be an answer of its own. */}
                  <CommandItem
                    forceMount
                    value={OTHER_ROW}
                    disabled={typed.length === 0}
                    data-checked={
                      pinned.length > 0 && !(models ?? []).some((entry) => entry.id === pinned)
                        ? "true"
                        : "false"
                    }
                    onSelect={() => pick(search)}
                  >
                    <span className="flex min-w-0 flex-col">
                      <span className="truncate">
                        {typed.length > 0 ? (
                          <>
                            Other — run <span className="font-mono">{typed}</span>
                          </>
                        ) : (
                          "Other… — type an id above"
                        )}
                      </span>
                      <span
                        className={cn(
                          "line-clamp-2 text-xs leading-snug",
                          typedError ? "text-destructive" : "text-muted-foreground",
                        )}
                      >
                        {typedError ??
                          "The agent CLI and, after a “:”, the model of it, handed over as typed."}
                      </span>
                    </span>
                  </CommandItem>
                </CommandGroup>
              </CommandList>
            </Command>
            <EffortStrip
              choices={choices}
              value={effort}
              name={effortName}
              onChange={(next) => onChange({ model, effort: next })}
            />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  )
}

/**
 * The pin as the trigger reads it: a sentence rather than two boxes.
 *
 * The agent CLI is the muted half — it is also the heading the model was
 * picked under — and the model is the part that identifies the choice, so
 * that is what survives the truncation of a narrow row.
 */
function TriggerLabel({
  model,
  effort,
  fallback,
  unpinnedLabel,
}: {
  model: string
  effort: string
  fallback: { model: string | null; effort: string | null } | null
  unpinnedLabel: string
}) {
  if (model.length === 0) {
    if (!fallback) return <span className="text-muted-foreground">{unpinnedLabel}</span>
    return (
      <span className="text-muted-foreground">
        Profile's own —{" "}
        {effort.length > 0 ? (
          // An effort of this slot's own over a model that is not: what runs is
          // the profile's *model*, at this effort, so the profile's own effort
          // is not what it is run at and has no business on the line.
          <>
            <span className="font-mono">{modelRefLabel(fallback.model)}</span> ·{" "}
            <span className="font-mono text-foreground">{effort}</span>
          </>
        ) : (
          <span className="font-mono">{pinLabel(fallback.model, fallback.effort)}</span>
        )}
      </span>
    )
  }
  const ref = parseModelRef(model)
  // Text the daemon will judge: shown as typed, since nothing here can split
  // an id it does not recognise.
  if (!ref) return <span className="font-mono">{model}</span>
  return (
    <>
      <span className="text-muted-foreground">{agentKindLabel(ref.agentKind)} </span>
      <span className="font-mono">{ref.model ?? DEFAULT_MODEL_LABEL}</span>
      {effort.length > 0 ? (
        <>
          <span className="text-muted-foreground"> · </span>
          <span className="font-mono">{effort}</span>
        </>
      ) : null}
    </>
  )
}

/**
 * What a search matches beyond the id: the blurb, and the task shapes the
 * catalog says this entry is and is not for — so typing "review" or "design"
 * finds a model by what it is good at, not only by its name.
 */
function modelKeywords(entry: ModelDto): string[] | undefined {
  const words = [entry.description, ...entry.best_for, ...entry.avoid_for].filter(
    (word): word is string => Boolean(word),
  )
  return words.length > 0 ? words : undefined
}

/**
 * The row's tooltip: what the catalog says this entry is and is not the
 * choice for, one line each — kept off the row itself so a model with a long
 * list of either stays the same three lines as one with none.
 */
function modelTitle(entry: ModelDto): string | undefined {
  const lines = [
    entry.best_for.length > 0 ? `best for: ${entry.best_for.join(", ")}` : null,
    entry.avoid_for.length > 0 ? `avoid for: ${entry.avoid_for.join(", ")}` : null,
  ].filter((line): line is string => line !== null)
  return lines.length > 0 ? lines.join("\n") : undefined
}

/**
 * Tier, cost and speed, compact enough to sit beside the id: a pill is left
 * out rather than shown empty where the catalog does not know it, which for
 * an opencode model nothing has been written about is all three.
 */
function ModelPills({ entry }: { entry: ModelDto }) {
  return (
    <span className="flex shrink-0 items-center gap-1">
      {entry.tier !== "unknown" ? <Pill>{entry.tier}</Pill> : null}
      {entry.cost !== null && entry.cost !== undefined ? <Pill>cost {entry.cost}/5</Pill> : null}
      {entry.speed !== null && entry.speed !== undefined ? (
        <Pill>speed {entry.speed}/5</Pill>
      ) : null}
    </span>
  )
}

function Pill({ children }: { children: ReactNode }) {
  return (
    <Badge
      variant="outline"
      className="h-4 rounded-sm px-1 py-0 text-[10px] font-normal leading-none"
    >
      {children}
    </Badge>
  )
}

/**
 * The efforts the chosen model can be run at, under the list that chose it.
 *
 * Radios rather than buttons carrying `role="radio"`: the group's keyboard —
 * Tab in, arrows within, and only the checked one in the tab order — is the
 * browser's own, and the strip is a portal away from the form it belongs to,
 * so nothing is submitted by their being inputs.
 */
function EffortStrip({
  choices,
  value,
  name,
  onChange,
}: {
  choices: EffortChoices
  value: string
  name: string
  onChange: (effort: string) => void
}) {
  if (choices.kind === "none") {
    return <p className="border-t px-3 py-2 text-xs text-muted-foreground">{choices.reason}</p>
  }

  if (choices.kind === "free") {
    return (
      <div className="flex items-center gap-2 border-t px-3 py-2">
        <span className="shrink-0 text-xs text-muted-foreground">Effort</span>
        <Input
          aria-label="Effort"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={AUTO_EFFORT_LABEL}
          autoComplete="off"
          spellCheck={false}
          className="h-7 font-mono"
        />
      </div>
    )
  }

  const items: { label: string; description: string | null; value: string }[] = [
    {
      // A choice, not a blank: the CLI's own level, named where the catalog
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
    <div
      role="radiogroup"
      aria-label="Effort"
      className="flex max-h-40 flex-col gap-0.5 overflow-y-auto border-t p-1"
    >
      {items.map((item) => (
        <label
          key={item.value || AUTO_EFFORT_LABEL}
          className="group flex cursor-pointer flex-col rounded-md px-2 py-1 text-xs transition-colors select-none has-checked:bg-primary has-checked:text-primary-foreground has-focus-visible:ring-3 has-focus-visible:ring-ring/50 hover:bg-muted has-checked:hover:bg-primary"
        >
          <input
            type="radio"
            name={name}
            className="sr-only"
            checked={value === item.value}
            onChange={() => onChange(item.value)}
          />
          <span className="truncate">{item.label}</span>
          {item.description ? (
            // Muted under the id, the same way a model's blurb sits under
            // its id above — checked or not, since the primary background
            // still needs the text readable over it.
            <span className="truncate text-[11px] leading-snug text-muted-foreground group-has-checked:text-primary-foreground/80">
              {item.description}
            </span>
          ) : null}
        </label>
      ))}
    </div>
  )
}

/** What can be offered beside a model, and why nothing can where nothing is. */
type EffortChoices =
  /** The efforts to offer, cheapest first, and what the CLI runs the model at. */
  | { kind: "options"; efforts: EffortDto[]; defaultEffort: string | null }
  /** Nothing knows the list: whatever is typed is passed on, as a model is. */
  | { kind: "free" }
  /** Nothing to choose, and the reason, which the strip shows as its hint. */
  | { kind: "none"; reason: string }

/**
 * What the chosen model can be run at, as the catalog says.
 *
 * `model` is the *effective* model — the one this slot will actually run on,
 * which for an empty pin is the profile's own — because that is the model the
 * daemon checks the effort against.
 */
function effortChoices(model: string, models: ModelDto[] | undefined): EffortChoices {
  // What is pinned is answered first, and without the catalog: that there is
  // no model to run an effort at is a fact about the pin, so the strip says so
  // while the catalog is still on its way rather than offering a free-text box
  // for an effort the daemon would refuse.
  const id = model.trim()
  if (id.length === 0) {
    return { kind: "none", reason: "An effort is run at a model — choose one first." }
  }
  const ref = parseModelRef(id)
  if (!ref) {
    return { kind: "none", reason: "An effort is run at a model — that one names no agent CLI." }
  }

  // No catalog — loading, or the endpoint failed — so nothing here can name a
  // list; the strip falls back to what the model itself is, free text.
  if (!models) return { kind: "free" }

  const entry = models.find((candidate) => candidate.id === id)
  // A model the catalog knows is held to its own efforts, empty ones included:
  // that is the model saying it takes none at all.
  if (entry && ref.model !== null) {
    if (entry.efforts.length === 0) {
      return { kind: "none", reason: `${id} takes no effort at all.` }
    }
    const defaultEffort = entry.efforts.find((effort) => effort.default)?.id ?? null
    return { kind: "options", efforts: entry.efforts, defaultEffort }
  }

  // An opencode model the catalog does not carry takes whichever variants it
  // was configured with, which only that model knows: its efforts are its own,
  // never its CLI's, so there is no list to hold it to and whatever is typed is
  // passed on — exactly what the daemon does with one (`known_efforts` is empty
  // for opencode).
  if (ref.agentKind === "opencode") return { kind: "free" }

  // Any other model the catalog does not carry, or an agent CLI on its own
  // default model — which one that is, is the CLI's own business. Everything
  // that CLI accepts, then, which is the union of what its models take. Which
  // model's own description that id belongs to is not asked here — the ids
  // are the only fact every model of the CLI agrees on — so the strip shows
  // these with no description line rather than one that may not fit the model
  // actually run.
  const union = unionOfEfforts(models, ref.agentKind)
  if (union.length === 0) return { kind: "free" }
  return {
    kind: "options",
    efforts: union.map((id) => ({ id, description: null, default: false })),
    defaultEffort: null,
  }
}

/** Every effort id the catalog lists for one agent CLI, in the order it lists them. */
function unionOfEfforts(models: ModelDto[], kind: ModelDto["agent_kind"]): string[] {
  const seen: string[] = []
  for (const model of models) {
    if (model.agent_kind !== kind) continue
    for (const effort of model.efforts) if (!seen.includes(effort.id)) seen.push(effort.id)
  }
  return seen
}
