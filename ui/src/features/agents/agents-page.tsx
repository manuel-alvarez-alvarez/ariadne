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

import { type AgentConfigDto, ApiError } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { AGENT_KIND_LABELS } from "@/lib/labels"
import { plural } from "@/lib/plural"

import { AgentFlagsDialog } from "./agent-flags-dialog"
import { sameFlags } from "./agent-flags-values"
import { agentConfigsQueryOptions } from "./queries"

const COLUMN_COUNT = 4

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

      {configs.isError ? (
        <ErrorState
          title="Could not load the agents"
          error={configs.error}
          // A daemon that never answered has nothing to say about why.
          description={
            ApiError.is(configs.error) && configs.error.isNetworkError
              ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
              : undefined
          }
          onRetry={() => void configs.refetch()}
        />
      ) : (
        <div className="overflow-hidden rounded-xl border">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead>Agent</TableHead>
                <TableHead>Extra flags</TableHead>
                <TableHead>Defaults</TableHead>
                <TableHead className="w-12 text-right">
                  <span className="sr-only">Actions</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {configs.isPending ? (
                <LoadingRows />
              ) : configs.data.length === 0 ? (
                <TableRow className="hover:bg-transparent">
                  <TableCell colSpan={COLUMN_COUNT} className="p-0">
                    <NoAgents />
                  </TableCell>
                </TableRow>
              ) : (
                configs.data.map((config) => (
                  <AgentRow
                    key={config.agent_kind}
                    config={config}
                    onEdit={() => openEdit(config)}
                  />
                ))
              )}
            </TableBody>
          </Table>
        </div>
      )}

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
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label={`Edit ${label} flags`}
          title={`Edit ${label} flags`}
          onClick={onEdit}
        >
          <PencilIcon />
        </Button>
      </TableCell>
    </TableRow>
  )
}

function LoadingRows() {
  return (
    <>
      {[0, 1, 2].map((row) => (
        <TableRow key={row} className="hover:bg-transparent">
          {Array.from({ length: COLUMN_COUNT }, (_, column) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: placeholder cells have no identity
            <TableCell key={column}>
              <Skeleton className="h-4 w-full" />
            </TableCell>
          ))}
        </TableRow>
      ))}
    </>
  )
}
