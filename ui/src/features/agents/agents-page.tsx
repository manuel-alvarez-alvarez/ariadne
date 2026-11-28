/**
 * The agents screen: each agent of the daemon's ACP registry, how it is
 * launched, and what it can be staffed on.
 *
 * The flags used to live on the profile that ran on the agent, which meant the
 * same flag written into every profile and drifting between them. They belong
 * to the agent, not to the persona, so there is one tab per registry agent
 * and nothing to create or delete — only the flag list to edit, and the
 * switches deciding which of that agent's models a plan may use.
 *
 * The models were a screen of their own, a flat list of every agent's catalog
 * at once with the agent repeated under each id. They are here instead
 * because the agent is half of what a model *is* — the id carries it — so the
 * two questions a reader has about an agent, "how does it run" and "what can
 * it run on", are answered in one place.
 *
 * A tab rather than a section per agent: catalogs stacked ran to a page and a
 * half of scrolling, and nothing on it wanted comparing across agents — a
 * model belongs to exactly one, and the gesture this screen exists for is
 * turning one agent's models on and off. One at a time is that gesture, and
 * the tab strip is also the list of agents there are, which the stacked
 * version made a reader scroll to find.
 *
 * The tabs are the daemon's own list, in its order, and each says whether its
 * flags still match what Ariadne ships, because that is the question a flag
 * list raises: what did I change here, and what would restoring put back.
 */

import { PencilIcon, RefreshCwIcon } from "lucide-react"
import { useEffect, useState } from "react"
import { useSearchParams } from "react-router-dom"
import { toast } from "sonner"
import { type AgentConfigDto, type Host, type HostRow, type ModelDto, useHostLists } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { HostListErrors } from "@/components/error-state"
import { HostDot } from "@/components/host-dot"
import { HostLabel } from "@/components/host-label"
import { PageHeader } from "@/components/page-header"
import { TabCount } from "@/components/tab-count"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { ModelTable } from "@/features/models/model-table"
import { modelsQueryOptions } from "@/features/models/queries"
import { useHostSelection } from "@/hooks/use-host-filter"
import { useRowsShowHost } from "@/hooks/use-host-label"
import { cn, describeError, plural } from "@/lib/format"
import { HOST_PARAM } from "@/routes/paths"

import { AgentFlagsDialog } from "./agent-flags-dialog"
import { sameFlags } from "./agent-flags-values"
import { AgentIcon } from "./agent-icon"
import { acpAgentsQueryOptions, agentConfigsQueryOptions, useRefreshAcpAgents } from "./queries"

/**
 * What refreshing does, in the one sentence it takes to say it — on the
 * button's tooltip and nowhere else, since there is no dialog to put it in.
 */
const REFRESH_HINT =
  "The daemon reprobes every agent, and picks up an agent installed since it started."

/** The params that open a detail panel, whose `?host=` is not the chip's. */
const PANEL_PARAMS = ["goal", "task", "session", "pr"]

export function AgentsPage() {
  // The dialog keeps its subject after closing so the exit animation still has
  // something to render; only `open` flips on close.
  const [editOpen, setEditOpen] = useState(false)
  const [editing, setEditing] = useState<HostRow<AgentConfigDto> | null>(null)
  // Undefined until the reader picks one: the first agent is the tab until
  // then, and that is not known before the list arrives.
  const [picked, setPicked] = useState<string>()
  // The host chip the reader picked, or a link asked for with `?host=`. It
  // belongs to the page, not to a tab, so it stays when the tab changes. A
  // `?host=` beside a panel param is that panel's host, not a pick of the chip.
  const [search] = useSearchParams()
  const hostParam = PANEL_PARAMS.some((one) => search.has(one)) ? null : search.get(HOST_PARAM)
  const [chip, setChip] = useState<Host | undefined>(hostParam ?? undefined)
  useEffect(() => {
    if (hostParam) setChip(hostParam)
  }, [hostParam])

  // The hosts the header picker selects: one host, or every reachable one.
  const selection = useHostSelection()
  const hosts = selection.read
  const configReads = useHostLists(hosts, agentConfigsQueryOptions)
  const configs = {
    ...configReads,
    isPending: selection.isPending || configReads.pending.length > 0,
  }
  const models = useHostLists(hosts, modelsQueryOptions)
  // The registry's icon URLs; the page reads fine without them, so a failed
  // request leaves the bundled icons and the letters, with no error of its own.
  const acpAgents = useHostLists(hosts, acpAgentsQueryOptions)
  const icons = new Map(acpAgents.rows.map((agent) => [agent.id, agent.icon]))

  function openEdit(config: HostRow<AgentConfigDto>) {
    setEditing(config)
    setEditOpen(true)
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Agents"
        description="The ACP agents Ariadne spawns sessions with. The flags are appended to every launch of that agent, whichever profile is running on it, and a model turned off is taken out of the catalog the orchestrator sizes a plan from."
        actions={null}
      />

      {/*
        The catalog failing is said once per host, above the tabs, rather than
        inside whichever one happens to be open: it is one request for every
        agent, and one thing went wrong. The flags are a separate request and
        still show.
      */}
      <HostListErrors
        what="the models"
        errors={models.errors}
        hostCount={hosts.length}
        hasRows={models.rows.length > 0}
        onRetry={() => models.refetch()}
      />

      {/* Each host whose agents failed to load is named, apart from the hosts
          that answered, which stay on screen — empty or not. */}
      <HostListErrors
        what="the agents"
        errors={configs.errors}
        hostCount={hosts.length}
        hasRows={configs.rows.length > 0}
        onRetry={() => configs.refetch()}
      />

      <AgentTabs
        hosts={hosts}
        configs={configs}
        icons={icons}
        models={models}
        chip={chip}
        onChip={setChip}
        picked={picked}
        onPick={setPicked}
        onEdit={openEdit}
      />

      {editing ? (
        <AgentFlagsDialog
          open={editOpen}
          onOpenChange={setEditOpen}
          config={editing}
          host={editing.host}
          showHost={hosts.length > 1}
        />
      ) : null}
    </div>
  )
}

/** What this page reads off the agent-config queries of the selected hosts. */
interface ConfigsQuery {
  pending: Host[]
  rows: HostRow<AgentConfigDto>[]
  errors: { host: Host; error: Error }[]
  isPending: boolean
  refetch: () => void
}

/** What this page reads off the model queries of the selected hosts. */
interface ModelsQuery {
  rows: HostRow<ModelDto>[]
  errors: { host: Host; error: Error }[]
  /** The hosts whose catalog has neither answered nor failed yet. */
  loading: Host[]
  refetch: () => void
}

function AgentTabs({
  hosts,
  configs,
  icons,
  models,
  chip,
  onChip,
  picked,
  onPick,
  onEdit,
}: {
  hosts: Host[]
  configs: ConfigsQuery
  icons: Map<string, string | null | undefined>
  models: ModelsQuery
  chip: Host | undefined
  onChip: (host: Host) => void
  picked: string | undefined
  onPick: (kind: string) => void
  onEdit: (config: HostRow<AgentConfigDto>) => void
}) {
  const refresh = useRefreshAcpAgents()
  // The count of models turned off is the shown host's, never a sum of hosts.
  const offOn = (host: Host | undefined) => ({
    host: hosts.length > 1 ? host : undefined,
    count:
      host === undefined ||
      models.loading.includes(host) ||
      models.errors.some((entry) => entry.host === host)
        ? undefined
        : models.rows.filter((model) => model.host === host && !model.enabled).length,
  })
  const fallbackOff = offOn(chip && hosts.includes(chip) ? chip : hosts[0])

  // Every selected host failed: the errors above say why, and Refresh stays.
  if (
    configs.rows.length === 0 &&
    configs.errors.length > 0 &&
    configs.errors.length === hosts.length
  ) {
    return (
      <div className="flex justify-end">
        <RefreshAgents refresh={refresh} hosts={hosts} off={fallbackOff} />
      </div>
    )
  }

  if (configs.isPending && configs.rows.length === 0) {
    return (
      <>
        <div className="flex justify-end">
          <RefreshAgents refresh={refresh} hosts={hosts} off={fallbackOff} />
        </div>
        {hosts.length ? (
          hosts.map((host) => (
            <Skeleton key={host} aria-label={`Loading agents on ${host}`} className="h-10 w-28" />
          ))
        ) : (
          <Skeleton aria-label="Loading agents" className="h-10 w-28" />
        )}
        <Skeleton className="h-96 rounded-xl" />
      </>
    )
  }
  if (configs.rows.length === 0) {
    return (
      <>
        <div className="flex justify-end">
          <RefreshAgents refresh={refresh} hosts={hosts} off={fallbackOff} />
        </div>
        <NoAgents />
      </>
    )
  }

  // One tab per agent id, in the order the hosts listed them; each carries
  // the hosts that have that agent.
  const byAgent = new Map<string, HostRow<AgentConfigDto>[]>()
  for (const config of configs.rows) {
    byAgent.set(config.agent_id, [...(byAgent.get(config.agent_id) ?? []), config])
  }
  const agents = [...byAgent.keys()]
  // The host an agent's tab shows: the chip's while the agent is on it, else
  // the first host that has the agent.
  const shownHost = new Map(
    agents.map((agent) => {
      const on = byAgent.get(agent) ?? []
      return [agent, (on.find((one) => one.host === chip) ?? on[0])?.host]
    }),
  )
  // The reader's choice while they have one, the first agent before
  // that — and again if the agent they were on stops being one of the answers.
  const current = picked && agents.includes(picked) ? picked : (agents[0] as string)
  const shown = (agent: string) => shownHost.get(agent)
  const hostLoading = (host: Host | undefined) =>
    host !== undefined && models.loading.includes(host)
  const hostFailed = (host: Host | undefined) => models.errors.some((e) => e.host === host)

  return (
    <Tabs value={current} onValueChange={(value) => onPick(value as string)}>
      <div className="flex items-center justify-between gap-2">
        <TabsList>
          {configs.pending.map((host) => (
            <Skeleton key={host} aria-label={`Loading agents on ${host}`} className="h-8 w-24" />
          ))}
          {agents.map((agent) => (
            <TabsTrigger
              key={agent}
              value={agent}
              icon={<AgentIcon agentId={agent} icon={icons.get(agent)} />}
              label={agent}
            >
              {/* How big that agent's catalog is — the rows behind the tab, which
                  is what this pill means everywhere else. Which of them are on
                  is the switch column's answer, not a number's. */}
              <TabCount
                count={
                  hostLoading(shown(agent)) || hostFailed(shown(agent))
                    ? undefined
                    : models.rows.filter(
                        (model) => model.agent_id === agent && model.host === shown(agent),
                      ).length
                }
                noun="model"
              />
            </TabsTrigger>
          ))}
        </TabsList>
        <RefreshAgents refresh={refresh} hosts={hosts} off={offOn(shown(current))} />
      </div>
      {agents.map((agent) => (
        <TabsContent key={agent} value={agent} className="pt-3">
          <AgentPanel
            configs={byAgent.get(agent) ?? []}
            // The catalog is one list per host; each tab takes its own agent's.
            models={models.rows.filter((model) => model.agent_id === agent)}
            modelsPending={hostLoading(shown(agent))}
            modelsError={models.errors.find((e) => e.host === shown(agent))?.error}
            onRetry={models.refetch}
            chip={chip}
            onChip={onChip}
            showHost={hosts.length > 1}
            onEdit={onEdit}
          />
        </TabsContent>
      ))}
    </Tabs>
  )
}

function RefreshAgents({
  refresh,
  hosts,
  off,
}: {
  refresh: ReturnType<typeof useRefreshAcpAgents>
  hosts: Host[]
  /** The models turned off on the host shown, named only where several are selected. */
  off: { host: Host | undefined; count: number | undefined }
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="outline"
            pending={refresh.isPending}
            onClick={() =>
              refresh.mutate(hosts, {
                onError: (error) =>
                  toast.error("Could not refresh agents", {
                    description: describeError(error),
                  }),
              })
            }
          />
        }
      >
        <RefreshCwIcon />
        Refresh
      </TooltipTrigger>
      <TooltipContent>
        {REFRESH_HINT}{" "}
        {off.count === undefined ? (
          <Skeleton aria-label="Loading model count" className="inline-block h-4 w-24" />
        ) : (
          `${plural(off.count, "model")} turned off`
        )}
        {off.host ? (
          <>
            {" "}
            on <HostLabel host={off.host} />
          </>
        ) : null}
        .
      </TooltipContent>
    </Tooltip>
  )
}

function NoAgents() {
  return (
    <EmptyState
      title="No agents"
      description="An agent config is the flag list an ACP agent is launched with. There is none to create — the daemon lists one per registry agent — so an empty list means it reported none; the flags can also be set with ariadne agent update."
    />
  )
}

function AgentPanel({
  configs,
  models,
  modelsPending,
  modelsError,
  onRetry,
  chip,
  onChip,
  showHost,
  onEdit,
}: {
  /** The agent's config on every host that has it. */
  configs: HostRow<AgentConfigDto>[]
  models: HostRow<ModelDto>[]
  modelsPending: boolean
  modelsError: Error | undefined
  onRetry: () => void
  /** The chip the reader picked: every edit below applies to its host. */
  chip: Host | undefined
  onChip: (host: Host) => void
  showHost: boolean
  onEdit: (config: HostRow<AgentConfigDto>) => void
}) {
  // A chip per host where the rows show their host: each names the one it edits.
  const showHosts = useRowsShowHost()
  const config = configs.find((one) => one.host === chip) ?? (configs[0] as HostRow<AgentConfigDto>)
  const label = config.agent_id
  const customized = !sameFlags(config.extra_flags, config.default_flags)

  return (
    <div className="flex flex-col gap-3">
      {showHosts ? (
        <fieldset aria-label="Host" className="flex flex-wrap items-center gap-1.5">
          {configs.map((one) => (
            <Button
              key={one.host}
              variant={one.host === config.host ? "secondary" : "outline"}
              size="sm"
              aria-pressed={one.host === config.host}
              className={cn("font-mono")}
              onClick={() => onChip(one.host)}
            >
              <HostDot host={one.host} />
              {one.host}
            </Button>
          ))}
        </fieldset>
      ) : null}
      <div className="flex items-center gap-2 overflow-x-auto">
        <span className="text-muted-foreground text-sm">Extra flags</span>
        {config.extra_flags.length > 0 ? (
          <div className="flex gap-1.5">
            {config.extra_flags.map((flag) => (
              <Badge key={flag} variant="outline" className="font-mono">
                {flag}
              </Badge>
            ))}
          </div>
        ) : (
          <span className="text-muted-foreground text-sm italic">
            none — Ariadne's own arguments only
          </span>
        )}
        {/* Whether this list is still what Ariadne ships — the one thing a
            flag list does not say about itself — and the way to change it.
            Both to the trailing end, away from the flags: a pill beside the
            mono ones reads as another flag rather than as a word about them. */}
        {customized ? (
          <Badge variant="secondary" className="ms-auto">
            Customized
          </Badge>
        ) : (
          <Badge variant="outline" className="ms-auto text-muted-foreground">
            Default
          </Badge>
        )}
        <Button
          variant="ghost"
          size="sm"
          onClick={() => onEdit(config)}
          aria-label={`Edit ${label} flags`}
        >
          <PencilIcon />
          Edit flags
        </Button>
      </div>

      <ModelTable
        host={config.host}
        models={models.filter((model) => model.host === config.host)}
        isPending={modelsPending}
        error={modelsError}
        onRetry={onRetry}
        showHost={showHost}
      />
    </div>
  )
}
