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
import { useState } from "react"
import { toast } from "sonner"

import {
  type AgentConfigDto,
  ApiError,
  type Host,
  type HostRow,
  type ModelDto,
  useHostLists,
} from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { TabCount } from "@/components/tab-count"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { ModelTable } from "@/features/models/model-table"
import { modelsQueryOptions } from "@/features/models/queries"
import { useHostFilter } from "@/hooks/use-host-filter"
import { cn, describeError, plural } from "@/lib/format"

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

export function AgentsPage() {
  // The dialog keeps its subject after closing so the exit animation still has
  // something to render; only `open` flips on close.
  const [editOpen, setEditOpen] = useState(false)
  const [editing, setEditing] = useState<HostRow<AgentConfigDto> | null>(null)
  // Undefined until the reader picks one: the first agent is the tab until
  // then, and that is not known before the list arrives.
  const [picked, setPicked] = useState<string>()

  // The hosts the header picker selects: one host, or every reachable one.
  const hosts = useHostFilter()
  const configs = useHostLists(hosts, agentConfigsQueryOptions)
  const models = useHostLists(hosts, modelsQueryOptions)
  // The registry's icon URLs; the page reads fine without them, so a failed
  // request leaves the bundled icons and the letters, with no error of its own.
  const acpAgents = useHostLists(hosts, acpAgentsQueryOptions)
  const icons = new Map(acpAgents.rows.map((agent) => [agent.id, agent.icon]))

  function openEdit(config: HostRow<AgentConfigDto>) {
    setEditing(config)
    setEditOpen(true)
  }

  const off = models.rows.filter((model) => !model.enabled).length

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
      {models.errors.map(({ host, error }) => (
        <ErrorState
          key={host}
          title={
            hosts.length > 1 ? `Could not load the models of ${host}` : "Could not load the models"
          }
          error={error}
          description={networkHint(error)}
          onRetry={() => models.refetch()}
        />
      ))}

      {/* A host whose agents failed to load is said apart from the hosts that
          answered, which stay on screen; with none answered the tabs say it. */}
      {configs.rows.length > 0
        ? configs.errors.map(({ host, error }) => (
            <ErrorState
              key={host}
              title={`Could not load the agents of ${host}`}
              error={error}
              description={networkHint(error)}
              onRetry={() => configs.refetch()}
            />
          ))
        : null}

      <AgentTabs
        hosts={hosts}
        configs={configs}
        icons={icons}
        models={models.rows}
        modelsPending={models.isPending && models.errors.length === 0}
        off={off}
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
        />
      ) : null}
    </div>
  )
}

/** A daemon that never answered has nothing to say about why. */
function networkHint(error: unknown): string | undefined {
  return ApiError.is(error) && error.isNetworkError
    ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
    : undefined
}

/** What this page reads off the agent-config queries of the selected hosts. */
interface ConfigsQuery {
  rows: HostRow<AgentConfigDto>[]
  errors: { host: Host; error: Error }[]
  isPending: boolean
  refetch: () => void
}

function AgentTabs({
  hosts,
  configs,
  icons,
  models,
  modelsPending,
  off,
  picked,
  onPick,
  onEdit,
}: {
  hosts: Host[]
  configs: ConfigsQuery
  icons: Map<string, string | null | undefined>
  models: HostRow<ModelDto>[]
  modelsPending: boolean
  off: number
  picked: string | undefined
  onPick: (kind: string) => void
  onEdit: (config: HostRow<AgentConfigDto>) => void
}) {
  const refresh = useRefreshAcpAgents()

  const [firstError] = configs.errors
  if (configs.rows.length === 0 && firstError) {
    return (
      <>
        <div className="flex justify-end">
          <RefreshAgents refresh={refresh} hosts={hosts} off={off} />
        </div>
        <ErrorState
          title="Could not load the agents"
          error={firstError.error}
          description={networkHint(firstError.error)}
          onRetry={() => configs.refetch()}
        />
      </>
    )
  }

  if (configs.isPending) {
    return (
      <>
        <div className="flex justify-end">
          <RefreshAgents refresh={refresh} hosts={hosts} off={off} />
        </div>
        <Skeleton className="h-96 rounded-xl" />
      </>
    )
  }
  if (configs.rows.length === 0) {
    return (
      <>
        <div className="flex justify-end">
          <RefreshAgents refresh={refresh} hosts={hosts} off={off} />
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
  const firstHost = new Map(agents.map((agent) => [agent, byAgent.get(agent)?.[0]?.host]))
  // The reader's choice while they have one, the first agent before
  // that — and again if the agent they were on stops being one of the answers.
  const current = picked && agents.includes(picked) ? picked : (agents[0] as string)

  return (
    <Tabs value={current} onValueChange={(value) => onPick(value as string)}>
      <div className="flex items-center justify-between gap-2">
        <TabsList>
          {agents.map((agent) => (
            <TabsTrigger key={agent} value={agent}>
              <AgentIcon agentId={agent} icon={icons.get(agent)} />
              {agent}
              {/* How big that agent's catalog is — the rows behind the tab, which
                  is what this pill means everywhere else. Which of them are on
                  is the switch column's answer, not a number's. */}
              <TabCount
                count={
                  modelsPending
                    ? undefined
                    : models.filter(
                        (model) => model.agent_id === agent && model.host === firstHost.get(agent),
                      ).length
                }
                noun="model"
              />
            </TabsTrigger>
          ))}
        </TabsList>
        <RefreshAgents refresh={refresh} hosts={hosts} off={off} />
      </div>
      {agents.map((agent) => (
        <TabsContent key={agent} value={agent} className="pt-3">
          <AgentPanel
            configs={byAgent.get(agent) ?? []}
            showHosts={hosts.length > 1}
            // The catalog is one list per host; each tab takes its own agent's.
            models={models.filter((model) => model.agent_id === agent)}
            modelsPending={modelsPending}
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
  off: number
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
        {REFRESH_HINT} {plural(off, "model")} turned off.
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
  showHosts,
  models,
  modelsPending,
  onEdit,
}: {
  /** The agent's config on every host that has it. */
  configs: HostRow<AgentConfigDto>[]
  /** Whether the selection spans hosts, so each chip names the one it edits. */
  showHosts: boolean
  models: HostRow<ModelDto>[]
  modelsPending: boolean
  onEdit: (config: HostRow<AgentConfigDto>) => void
}) {
  // The chip the reader picked: every edit below applies to its host.
  const [chip, setChip] = useState<Host>()
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
              onClick={() => setChip(one.host)}
            >
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
      />
    </div>
  )
}
