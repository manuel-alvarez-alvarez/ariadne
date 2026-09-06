/**
 * The models screen: everything an agent can be staffed on, and which of them
 * the user allows.
 *
 * The catalog is not the user's to edit — it is what each agent CLI ships,
 * plus what `opencode models --verbose` discovers — so there is nothing here
 * to create and nothing to delete. What there is, is one switch per row: a
 * model turned off is refused as a pin and never offered to an orchestrator
 * sizing a plan, while work already staffed on it keeps running, because a
 * pin is the snapshot a task was created with rather than a lookup.
 *
 * Rows are grouped by agent CLI, in the daemon's order, because the CLI is
 * half of what a model *is*: the id carries it, and turning a whole CLI off
 * is the common gesture — one section at a time rather than one row at a
 * time scattered through a flat list.
 *
 * A disabled row stays where it is, greyed, rather than moving to a section
 * of its own: what a reader came for is "is this one on", and a row that
 * jumps when it is toggled is a row they then have to find again.
 */

import { useQuery } from "@tanstack/react-query"
import { toast } from "sonner"

import type { AgentKind, ModelDto } from "@/api"
import { DataTable } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { Switch } from "@/components/ui/switch"
import { TableCell, TableRow } from "@/components/ui/table"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { AGENT_KIND_LABELS, cn, plural } from "@/lib/format"

import { modelsQueryOptions, useSetModelEnabled } from "./queries"

const COLUMNS = [
  { header: "Model" },
  { header: "Tier" },
  { header: "What it is for" },
  // The switch column is headed by what it decides, not by "Enabled": the
  // question a reader is answering here is whether a plan may use the row.
  { className: "w-24 text-right", header: "Available" },
]

export function ModelsPage() {
  const models = useQuery(modelsQueryOptions())
  const off = models.data?.filter((model) => !model.enabled).length ?? 0

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Models"
        description="What Ariadne can staff an agent on. Turning one off takes it out of the catalog the orchestrator sizes a plan from, and refuses it as a pin; work already running on it is left alone."
      />

      {models.data ? (
        <p className="text-sm text-muted-foreground">
          {plural(models.data.length, "model")}
          {off > 0 ? `, ${off} turned off` : null}
        </p>
      ) : null}

      <DataTable
        query={models}
        errorTitle="Could not load the models"
        columns={COLUMNS}
        empty={<NoModels />}
        rowKey={(model) => model.id}
        renderRow={(model) => <ModelRow model={model} />}
      />
    </div>
  )
}

function NoModels() {
  return (
    <EmptyState
      className="border-0 py-12"
      title="No models"
      description="The catalog is what each agent CLI ships, so an empty one means the daemon reported none. The same list is ariadne models ls."
    />
  )
}

function ModelRow({ model }: { model: ModelDto }) {
  const setEnabled = useSetModelEnabled()
  const label = AGENT_KIND_LABELS[model.agent_kind as AgentKind] ?? model.agent_kind

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
    <TableRow className={cn(!model.enabled && "text-muted-foreground")}>
      <TableCell>
        <div className="flex flex-col gap-0.5">
          <span className="font-mono text-xs">{model.id}</span>
          <span className="text-xs text-muted-foreground">{label}</span>
        </div>
      </TableCell>
      <TableCell>
        <Badge variant={model.enabled ? "secondary" : "outline"}>{model.tier}</Badge>
      </TableCell>
      <TableCell className="whitespace-normal">
        {model.description ?? <span className="text-muted-foreground italic">nothing known</span>}
      </TableCell>
      <TableCell className="text-right">
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
