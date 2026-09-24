/**
 * The model catalog of one agent: what it can be staffed on, which of those
 * the user allows, and how each one ranks against the rest.
 *
 * The catalog is not the user's to edit — it is what discovery found each
 * agent offering — so there is nothing here to create and nothing to delete. What there is, is one switch per row: a
 * model turned off is refused as a pin and never offered to an orchestrator
 * sizing a plan, while work already staffed on it keeps running, because a
 * pin is the snapshot a task was created with rather than a lookup.
 *
 * The rank beside it is a second, independent decision: frontier, balanced,
 * fast or local, so the orchestrator staffs the smallest one a task earns
 * rather than always reaching for the strongest model in the catalog. An
 * unranked model stays usable — a fresh install still works — with a ranked
 * one preferred over it.
 *
 * A table per agent rather than one flat list, because the agent is half of
 * what a model *is*: the id carries it, and turning a whole agent off is the
 * common gesture. The section around each of these is the agent it belongs to
 * (see `features/agents/agents-page.tsx`), which is why no row repeats the
 * agent's name — its heading has already said it.
 *
 * A disabled row stays where it is, greyed, rather than moving to a section
 * of its own: what a reader came for is "is this one on", and a row that
 * jumps when it is toggled is a row they then have to find again.
 */

import { toast } from "sonner"

import type { ModelDto, ModelRank } from "@/api"
import { ScrollableTable } from "@/components/scroll-edge"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

import { useSetModelEnabled, useSetModelRank } from "./queries"

/** Rows drawn while the catalog loads. Enough to read as a table, not as a page. */
const PLACEHOLDER_ROWS = [0, 1, 2]

/** The value the picker sends for a model with no rank. Not `""`: a select's
 * value is never the empty string, and `null` is not a valid item value. */
const UNRANKED = "unranked"

/**
 * The four ranks a model can be given, in ladder order, and the one line each
 * means on screen — the picker's own list is where a reader learns what they
 * are, since there is nowhere else on this screen to put it.
 *
 * `local` is last and out of ladder order on purpose: it says where a model
 * runs rather than how capable it is, so the orchestrator never reaches for
 * it on its own — only a pin that names it does.
 */
const RANKS: { value: ModelRank; label: string; meaning: string }[] = [
  { value: "frontier", label: "Frontier", meaning: "the strongest model, when a task earns it" },
  { value: "balanced", label: "Balanced", meaning: "the default for most tasks" },
  { value: "fast", label: "Fast", meaning: "the cheapest model that still earns a task" },
  {
    value: "local",
    label: "Local",
    meaning: "off the ladder — staffed only where a task names it",
  },
]

const RANK_ITEMS = [
  { value: UNRANKED, label: "Unranked" },
  ...RANKS.map(({ value, label }) => ({ value, label })),
]

/**
 * The switch column, held against the trailing edge.
 *
 * A table of switches is read down that column, and it is the only part of a
 * row there is anything to *do* with — so it is the part that must not slide
 * off a narrow window, and must not sit under the fade that says the rest of
 * the row has. `bg-inherit` rather than a colour of its own, so the row's own
 * background — including the one hover paints — carries through it instead of
 * a solid stripe down the edge of the table.
 *
 * `pe-3` rather than the cell's usual `p-2`, because a switch is wider than it
 * looks: its hit target is an `::after` reaching twelve pixels past each side,
 * and a pseudo-element still counts towards a scrollport's `scrollWidth` even
 * though it is not an element anyone can measure. Eight pixels of padding left
 * four of it hanging over the edge, which is a table that scrolls sideways by
 * three pixels at every width — the horizontal scrollbar that would not go
 * away. Twelve pixels of padding is exactly the reach, so it lands on the edge
 * instead of past it, and the hit target keeps its size.
 */
const PINNED = "sticky right-0 z-20 bg-inherit pe-3"

export function ModelTable({ models, isPending }: { models: ModelDto[]; isPending: boolean }) {
  return (
    <ScrollableTable className="rounded-lg border" pinnedEnd>
      <TableHeader>
        <TableRow className="bg-background hover:bg-transparent">
          {/*
            The id is the widest thing here and the one that may wrap, so it is
            the column that gives: everything after it is sized to its content
            and would only be pushed off the edge instead.
          */}
          <TableHead>Model</TableHead>
          <TableHead>What it is for</TableHead>
          <TableHead>Rank</TableHead>
          {/*
            Headed by what it decides, not by "Enabled": the question a reader
            is answering here is whether a plan may use the row.
          */}
          <TableHead className={cn("w-24 text-right", PINNED)}>Available</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {isPending ? (
          PLACEHOLDER_ROWS.map((row) => (
            <TableRow key={row} className="bg-background hover:bg-transparent">
              {[0, 1, 2, 3].map((cell) => (
                <TableCell key={cell}>
                  <Skeleton className="h-4 w-full" />
                </TableCell>
              ))}
            </TableRow>
          ))
        ) : models.length ? (
          models.map((model) => <ModelRow key={model.id} model={model} />)
        ) : (
          <TableRow className="bg-background hover:bg-transparent">
            <TableCell colSpan={4} className="py-6 text-center text-muted-foreground text-sm">
              No models — the daemon reported none for this agent. The same catalog is{" "}
              <span className="font-mono text-xs">ariadne models ls</span>.
            </TableCell>
          </TableRow>
        )}
      </TableBody>
    </ScrollableTable>
  )
}

function ModelRow({ model }: { model: ModelDto }) {
  const setEnabled = useSetModelEnabled()

  function toggle(enabled: boolean) {
    setEnabled.mutate(
      { id: model.id, enabled },
      {
        onSuccess: () =>
          toast.success(enabled ? "Model turned on" : "Model turned off", {
            description: model.id,
          }),
        // The daemon refuses the last model left on, and says so by name.
        // Said here rather than swallowed: the switch springs back, and
        // without a word that reads as a click that did not register.
        onError: (error: Error) =>
          toast.error("Could not change the model", { description: error.message }),
      },
    )
  }

  return (
    <TableRow className={cn("bg-background", !model.enabled && "text-muted-foreground")}>
      {/*
        `break-all` rather than the cell's default `nowrap`: an opencode id runs
        to fifty characters with no space in it, and a cell that cannot break
        one sets the table's minimum width to however long the longest id is —
        which is what had every screen narrower than that scrolling sideways.
      */}
      <TableCell className="whitespace-normal">
        <span className="break-all font-mono text-xs">{model.id}</span>
      </TableCell>
      {/*
        `wrap-anywhere` for the same reason, on the other end of it: a
        description that names a model in parentheses carries a forty-character
        word, and wrapping between words alone cannot get below it.

        Not `break-words`, which is `overflow-wrap: break-word` and breaks such
        a word only once the column is already too narrow for it — it does not
        lower the column's *min-content* width, so the table's minimum stays
        that word wide and the last few pixels still scroll. `wrap-anywhere` is
        `overflow-wrap: anywhere`, which counts in that measurement.
      */}
      <TableCell className="whitespace-normal wrap-anywhere">
        {model.description ?? <span className="text-muted-foreground italic">nothing known</span>}
      </TableCell>
      <TableCell>
        <RankPicker model={model} />
      </TableCell>
      <TableCell className={cn("text-right", PINNED)}>
        <Tooltip>
          <TooltipTrigger
            render={
              <Switch
                checked={model.enabled}
                disabled={setEnabled.isPending}
                onCheckedChange={toggle}
                // The row's own name is not enough: a screen of switches is
                // read down the column, and every one of them would be
                // "Available" without the model it is about.
                aria-label={`${model.id} available`}
              />
            }
          />
          <TooltipContent>
            {model.enabled
              ? "An agent can be staffed on this model"
              : "Turned off: not offered, and refused as a pin"}
          </TooltipContent>
        </Tooltip>
      </TableCell>
    </TableRow>
  )
}

/**
 * What the user ranks this model, against the others its agent offers — or
 * clears back to unranked. The list is where the meaning of each rank is
 * said, since there is nowhere else on this screen to put it.
 */
function RankPicker({ model }: { model: ModelDto }) {
  const setRank = useSetModelRank()

  function change(value: string | null) {
    const rank = value === null || value === UNRANKED ? null : (value as ModelRank)
    setRank.mutate(
      { id: model.id, rank },
      {
        // Said the same way a refused enable/disable is: the picker springs
        // back to the daemon's own answer, and without a word that reads as
        // a pick that did not register.
        onError: (error: Error) =>
          toast.error("Could not change the rank", { description: error.message }),
      },
    )
  }

  return (
    <Select
      value={model.rank ?? UNRANKED}
      onValueChange={change}
      disabled={setRank.isPending}
      // Without this the trigger shows the stored value (`fast`) rather than
      // the option's label ("Fast").
      items={RANK_ITEMS}
    >
      <SelectTrigger
        size="sm"
        className="w-32"
        // The row's own name is not enough: a screen of pickers is read down
        // the column, and every one of them would be "Rank" without the
        // model it is about.
        aria-label={`${model.id} rank`}
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={UNRANKED}>Unranked</SelectItem>
        {RANKS.map((rank) => (
          <SelectItem key={rank.value} value={rank.value}>
            <span className="flex flex-col py-0.5">
              <span>{rank.label}</span>
              <span className="text-muted-foreground text-xs">{rank.meaning}</span>
            </span>
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
