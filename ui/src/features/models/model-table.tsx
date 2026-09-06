/**
 * The model catalog of one agent CLI: what it can be staffed on, and which of
 * those the user allows.
 *
 * The catalog is not the user's to edit — it is what each agent CLI ships,
 * plus what `opencode models --verbose` discovers — so there is nothing here
 * to create and nothing to delete. What there is, is one switch per row: a
 * model turned off is refused as a pin and never offered to an orchestrator
 * sizing a plan, while work already staffed on it keeps running, because a
 * pin is the snapshot a task was created with rather than a lookup.
 *
 * A table per CLI rather than one flat list, because the CLI is half of what a
 * model *is*: the id carries it, and turning a whole CLI off is the common
 * gesture. The section around each of these is the agent it belongs to (see
 * `features/agents/agents-page.tsx`), which is why no row repeats the CLI's
 * name — its heading has already said it.
 *
 * A disabled row stays where it is, greyed, rather than moving to a section
 * of its own: what a reader came for is "is this one on", and a row that
 * jumps when it is toggled is a row they then have to find again.
 */

import { toast } from "sonner"

import type { ModelDto } from "@/api"
import { ScrollableTable } from "@/components/scroll-edge"
import { Badge } from "@/components/ui/badge"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { cn } from "@/lib/format"

import { useSetModelEnabled } from "./queries"

/** Rows drawn while the catalog loads. Enough to read as a table, not as a page. */
const PLACEHOLDER_ROWS = [0, 1, 2]

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
          <TableHead className="w-20">Tier</TableHead>
          <TableHead>What it is for</TableHead>
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
              No models — the daemon reported none for this CLI. The same catalog is{" "}
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
      <TableCell>
        <Badge variant={model.enabled ? "secondary" : "outline"}>{model.tier}</Badge>
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
