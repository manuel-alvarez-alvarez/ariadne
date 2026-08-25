/**
 * The agents screen: how each coding-agent CLI is launched.
 *
 * The flags used to live on the profile that ran on the agent, which meant the
 * same `--dangerously-skip-permissions` written into every Claude Code profile
 * and drifting between them. They belong to the CLI, not to the persona, so
 * this screen is one row per agent kind and nothing else — there is nothing to
 * create and nothing to delete, only the flag list to edit.
 *
 * The rows are the daemon's own list, in its order, and every one of them says
 * whether it still matches what Ariadne ships, because that is the question a
 * flag list raises: what did I change here, and what would restoring put back.
 */

import { useQuery } from "@tanstack/react-query"
import { PencilIcon } from "lucide-react"
import { useState } from "react"

import type { AgentConfigDto } from "@/api"
import { DataTable, RowAction } from "@/components/data-table"
import { EmptyState } from "@/components/empty-state"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { TableCell, TableRow } from "@/components/ui/table"
import { AGENT_KIND_LABELS, plural } from "@/lib/format"
import { AgentFlagsDialog } from "./agent-flags-dialog"
import { sameFlags } from "./agent-flags-values"
import { agentConfigsQueryOptions } from "./queries"

const COLUMNS = [
  { header: "Agent" },
  { header: "Extra flags" },
  { header: "Defaults" },
  { className: "w-12 text-right" },
]

export function AgentsPage() {
  // The dialog keeps its subject after closing so the exit animation still has
  // something to render; only `open` flips on close.
  const [editOpen, setEditOpen] = useState(false)
  const [editing, setEditing] = useState<AgentConfigDto | null>(null)

  const configs = useQuery(agentConfigsQueryOptions())

  function openEdit(config: AgentConfigDto) {
    setEditing(config)
    setEditOpen(true)
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Agents"
        description="The coding-agent CLIs Ariadne spawns sessions with. The flags here are appended to every launch of that agent, whichever profile is running on it."
      />

      {configs.data ? (
        <p className="text-sm text-muted-foreground">{plural(configs.data.length, "agent")}</p>
      ) : null}

      <DataTable
        query={configs}
        errorTitle="Could not load the agents"
        columns={COLUMNS}
        empty={<NoAgents />}
        rowKey={(config) => config.agent_kind}
        renderRow={(config) => <AgentRow config={config} onEdit={() => openEdit(config)} />}
      />

      <AgentFlagsDialog open={editOpen} onOpenChange={setEditOpen} config={editing} />
    </div>
  )
}

function NoAgents() {
  return (
    <EmptyState
      // The table's own frame is the box here.
      className="border-0 py-12"
      title="No agents"
      description="An agent config is the flag list a coding-agent CLI is launched with. There is none to create — the daemon ships one per agent kind — so an empty list means it reported none; the flags can also be set with ariadne agent update."
    />
  )
}

function AgentRow({ config, onEdit }: { config: AgentConfigDto; onEdit: () => void }) {
  const label = AGENT_KIND_LABELS[config.agent_kind]
  const customized = !sameFlags(config.extra_flags, config.default_flags)

  return (
    <TableRow>
      <TableCell className="font-medium">{label}</TableCell>
      <TableCell className="whitespace-normal">
        {config.extra_flags.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
            {config.extra_flags.map((flag) => (
              <Badge key={flag} variant="outline" className="font-mono">
                {flag}
              </Badge>
            ))}
          </div>
        ) : (
          <span className="text-muted-foreground italic">none — Ariadne's own arguments only</span>
        )}
      </TableCell>
      <TableCell>
        {customized ? (
          <Badge variant="secondary">Customized</Badge>
        ) : (
          <span className="text-muted-foreground">Unchanged</span>
        )}
      </TableCell>
      <TableCell className="text-right">
        <RowAction icon={<PencilIcon />} label={`Edit ${label} flags`} onClick={onEdit} />
      </TableCell>
    </TableRow>
  )
}
