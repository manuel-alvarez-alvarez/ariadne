/**
 * The effort field: which reasoning level the model beside it is run at.
 *
 * The opposite of the {@link import("./model-picker").ModelPicker} it stands
 * next to, and for the opposite reason. A model is free text — the daemon
 * hands whatever is typed to the CLI it names — where an effort is a closed
 * list the daemon checks a write against, and the list belongs to the *model*:
 * `claude_code:claude-opus-5` takes five levels, `codex:gpt-5.6-luna` takes
 * five different ones, and `claude_code:claude-haiku-4-5` takes none at all.
 * So this is a select, scoped by whatever the model box holds, and free text
 * only where nothing can know the list — an opencode model discovery has not
 * seen, whose efforts are the variant names *that* model was configured with
 * and never its CLI's, so no other opencode entry can stand in for them.
 *
 * The first entry is `auto`, which is no effort pinned: the CLI runs the model
 * at its own default, named in the label where the catalog says what it is
 * (`auto (high)`). That is a choice, not a blank, which is why it is an option
 * rather than an empty box.
 *
 * The rules here are the daemon's, said before the round trip rather than
 * after it (`ariadne_core::models::effort_error`, `http/pins.rs`): a model the
 * catalog lists is held to its own efforts, one it does not to everything its
 * CLI accepts, an effort with no model to run at is refused outright, and an
 * effort that the model moved to does not take is dropped back to auto — which
 * is exactly what the daemon does with a pin whose model changes and whose
 * effort does not travel with it.
 */

import { useEffect, useMemo } from "react"

import type { ModelDto } from "@/api"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { cn } from "@/lib/format"

import { parseModelRef } from "./model-ref"

/** What an unpinned effort is called, in the list and on the trigger. */
const AUTO_LABEL = "auto"

/**
 * The auto option's value inside the select, since the field's own value for
 * it is the empty string and a select item has to be told something.
 */
const AUTO_VALUE = "__auto__"

/** What can be offered beside a model, and why nothing can where nothing is. */
type EffortChoices =
  /** The efforts to offer, cheapest first, and what the CLI runs the model at. */
  | { kind: "options"; efforts: string[]; defaultEffort: string | null }
  /** Nothing knows the list: whatever is typed is passed on, as a model is. */
  | { kind: "free" }
  /** Nothing to choose, and the reason, which the field shows as its hint. */
  | { kind: "none"; reason: string }

/**
 * What the model in the box can be run at, as the catalog says.
 *
 * `model` is the *effective* model — the one this slot will actually run on,
 * which for the forms that override a profile is the profile's own where the
 * box is empty — because that is the model the daemon checks the effort
 * against.
 */
function effortChoices(model: string, models: ModelDto[] | undefined): EffortChoices {
  // What the box beside it holds is answered first, and without the catalog:
  // that there is no model to run an effort at is a fact about that box, so
  // the field says so while the catalog is still on its way rather than
  // offering a free-text box for an effort the daemon would refuse.
  const id = model.trim()
  if (id.length === 0) {
    return { kind: "none", reason: "An effort is run at a model — choose one first." }
  }
  const ref = parseModelRef(id)
  if (!ref) {
    return { kind: "none", reason: "An effort is run at a model — that one names no agent CLI." }
  }

  // No catalog — loading, or the endpoint failed — so nothing here can name a
  // list; the field falls back to what the model box itself is, free text.
  if (!models) return { kind: "free" }

  const entry = models.find((candidate) => candidate.id === id)
  // A model the catalog knows is held to its own efforts, empty ones included:
  // that is the model saying it takes none at all.
  if (entry && ref.model !== null) {
    if (entry.efforts.length === 0) {
      return { kind: "none", reason: `${id} takes no effort at all.` }
    }
    return { kind: "options", efforts: entry.efforts, defaultEffort: entry.default_effort ?? null }
  }

  // An opencode model the catalog does not carry takes whichever variants it
  // was configured with, which only that model knows: its efforts are its own,
  // never its CLI's, so there is no list to hold it to and whatever is typed is
  // passed on — exactly what the daemon does with one (`known_efforts` is empty
  // for opencode).
  if (ref.agentKind === "opencode") return { kind: "free" }

  // Any other model the catalog does not carry, or an agent CLI on its own
  // default model — which one that is, is the CLI's own business. Everything
  // that CLI accepts, then, which is the union of what its models take.
  const union = unionOfEfforts(models, ref.agentKind)
  if (union.length === 0) return { kind: "free" }
  return { kind: "options", efforts: union, defaultEffort: null }
}

/** Every effort the catalog lists for one agent CLI, in the order it lists them. */
function unionOfEfforts(models: ModelDto[], kind: ModelDto["agent_kind"]): string[] {
  const seen: string[] = []
  for (const model of models) {
    if (model.agent_kind !== kind) continue
    for (const effort of model.efforts) if (!seen.includes(effort)) seen.push(effort)
  }
  return seen
}

export function EffortPicker({
  value,
  onChange,
  model,
  models,
  label = "Effort",
  invalid,
  className,
}: {
  /** The pinned effort, or the empty string for auto. */
  value: string
  onChange: (value: string) => void
  /** The model this effort runs at, as the slot will actually run it. */
  model: string
  /** The catalog, or undefined while it is loading or failed to load. */
  models: ModelDto[] | undefined
  /** The field's accessible name; several pickers on one form each need theirs. */
  label?: string
  invalid?: boolean
  className?: string
}) {
  const choices = useMemo(() => effortChoices(model, models), [model, models])

  // An effort belongs to the model it runs at, so a model moved out from under
  // one that does not take it leaves the field on auto — the daemon's own rule
  // for a repin whose effort does not travel with it, applied where it can
  // still be seen rather than refused on submit.
  useEffect(() => {
    if (value.length === 0) return
    if (
      choices.kind === "none" ||
      (choices.kind === "options" && !choices.efforts.includes(value))
    ) {
      onChange("")
    }
  }, [choices, value, onChange])

  if (choices.kind === "free") {
    return (
      <Input
        aria-label={label}
        aria-invalid={invalid ? true : undefined}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder={AUTO_LABEL}
        autoComplete="off"
        spellCheck={false}
        className={cn("font-mono", className)}
      />
    )
  }

  const disabled = choices.kind === "none"
  const efforts = choices.kind === "options" ? choices.efforts : []
  const auto =
    choices.kind === "options" && choices.defaultEffort
      ? `${AUTO_LABEL} (${choices.defaultEffort})`
      : AUTO_LABEL
  const items = [
    { label: auto, value: AUTO_VALUE },
    ...efforts.map((e) => ({ label: e, value: e })),
  ]

  return (
    <Select
      value={value || AUTO_VALUE}
      onValueChange={(picked) => onChange(picked === AUTO_VALUE ? "" : (picked ?? ""))}
      disabled={disabled}
      // Without this the trigger would show the stored value rather than the
      // option's label, which is how `auto` gets what the CLI runs it at.
      items={items}
    >
      <SelectTrigger
        aria-label={label}
        aria-invalid={invalid ? true : undefined}
        // The one place a disabled field can say why it is disabled: there is
        // no room for a line of its own beside a model box.
        title={disabled ? choices.reason : undefined}
        className={cn("w-full font-mono", className)}
      >
        <SelectValue placeholder={AUTO_LABEL} />
      </SelectTrigger>
      <SelectContent>
        {items.map((item) => (
          <SelectItem key={item.value} value={item.value}>
            {item.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
