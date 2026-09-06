/**
 * The agents screen: each coding-agent CLI, how it is launched, and what it
 * can be staffed on.
 *
 * The flags used to live on the profile that ran on the agent, which meant the
 * same `--dangerously-skip-permissions` written into every Claude Code profile
 * and drifting between them. They belong to the CLI, not to the persona, so
 * there is one tab per agent kind and nothing to create or delete — only the
 * flag list to edit, and the switches deciding which of that CLI's models a
 * plan may use.
 *
 * The models were a screen of their own, a flat list of every CLI's catalog at
 * once with the CLI repeated under each id. They are here instead because the
 * CLI is half of what a model *is* — the id carries it — so the two questions
 * a reader has about an agent, "how does it run" and "what can it run on", are
 * answered in one place.
 *
 * A tab rather than a section per CLI: three catalogs stacked ran to a page
 * and a half of scrolling, and nothing on it wanted comparing across CLIs —
 * a model belongs to exactly one, and the gesture this screen exists for is
 * turning one CLI's models on and off. One at a time is that gesture, and the
 * tab strip is also the list of CLIs there are, which the stacked version made
 * a reader scroll to find.
 *
 * The tabs are the daemon's own list, in its order, and each says whether its
 * flags still match what Ariadne ships, because that is the question a flag
 * list raises: what did I change here, and what would restoring put back.
 */

import { useQuery } from "@tanstack/react-query"
import { PencilIcon } from "lucide-react"
import { useState } from "react"

import { type AgentConfigDto, type AgentKind, ApiError, type ModelDto } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { TabCount } from "@/components/tab-count"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ModelTable } from "@/features/models/model-table"
import { modelsQueryOptions } from "@/features/models/queries"
import { AGENT_KIND_LABELS, plural } from "@/lib/format"

import { AgentFlagsDialog } from "./agent-flags-dialog"
import { sameFlags } from "./agent-flags-values"
import { agentConfigsQueryOptions } from "./queries"

export function AgentsPage() {
  // The dialog keeps its subject after closing so the exit animation still has
  // something to render; only `open` flips on close.
  const [editOpen, setEditOpen] = useState(false)
  const [editing, setEditing] = useState<AgentConfigDto | null>(null)
  // Undefined until the reader picks one: the daemon's first CLI is the tab
  // until then, and that is not known before the list arrives.
  const [picked, setPicked] = useState<string>()

  const configs = useQuery(agentConfigsQueryOptions())
  const models = useQuery(modelsQueryOptions())

  function openEdit(config: AgentConfigDto) {
    setEditing(config)
    setEditOpen(true)
  }

  const off = models.data?.filter((model) => !model.enabled).length ?? 0

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Agents"
        description="The coding-agent CLIs Ariadne spawns sessions with. The flags are appended to every launch of that agent, whichever profile is running on it, and a model turned off is taken out of the catalog the orchestrator sizes a plan from."
      />

      {configs.data && models.data ? (
        <p className="text-sm text-muted-foreground">
          {plural(configs.data.length, "agent")}, {plural(models.data.length, "model")}
          {off > 0 ? `, ${off} turned off` : null}
        </p>
      ) : null}

      {/*
        The catalog failing is said once, above the tabs, rather than inside
        whichever one happens to be open: it is one request for every CLI, and
        one thing went wrong. The flags are a separate request and still show.
      */}
      {models.isError ? (
        <ErrorState
          title="Could not load the models"
          error={models.error}
          description={networkHint(models.error)}
          onRetry={() => void models.refetch()}
        />
      ) : null}

      <AgentTabs
        configs={configs}
        models={models.data ?? []}
        modelsPending={models.isPending && !models.isError}
        picked={picked}
        onPick={setPicked}
        onEdit={openEdit}
      />

      <AgentFlagsDialog open={editOpen} onOpenChange={setEditOpen} config={editing} />
    </div>
  )
}

/** A daemon that never answered has nothing to say about why. */
function networkHint(error: unknown): string | undefined {
  return ApiError.is(error) && error.isNetworkError
    ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
    : undefined
}

/** What this page reads off the agent-config query. */
interface ConfigsQuery {
  data?: AgentConfigDto[]
  isPending: boolean
  isError: boolean
  error: unknown
  refetch: () => unknown
}

function AgentTabs({
  configs,
  models,
  modelsPending,
  picked,
  onPick,
  onEdit,
}: {
  configs: ConfigsQuery
  models: ModelDto[]
  modelsPending: boolean
  picked: string | undefined
  onPick: (kind: string) => void
  onEdit: (config: AgentConfigDto) => void
}) {
  if (configs.isError) {
    return (
      <ErrorState
        title="Could not load the agents"
        error={configs.error}
        description={networkHint(configs.error)}
        onRetry={() => void configs.refetch()}
      />
    )
  }

  if (configs.isPending) return <Skeleton className="h-96 rounded-xl" />
  if (!configs.data?.length) return <NoAgents />

  // The reader's choice while they have one, the daemon's first CLI before
  // that — and again if the CLI they were on stops being one of the answers.
  const kinds = configs.data.map((config) => config.agent_kind)
  const current = picked && kinds.includes(picked as AgentKind) ? picked : kinds[0]

  return (
    <Tabs value={current} onValueChange={(value) => onPick(value as string)}>
      <TabsList>
        {configs.data.map((config) => (
          <TabsTrigger key={config.agent_kind} value={config.agent_kind}>
            {AGENT_KIND_LABELS[config.agent_kind] ?? config.agent_kind}
            {/* How big that CLI's catalog is — the rows behind the tab, which
                is what this pill means everywhere else. Which of them are on
                is the switch column's answer, not a number's. */}
            <TabCount
              count={
                modelsPending
                  ? undefined
                  : models.filter((model) => model.agent_kind === config.agent_kind).length
              }
              noun="model"
            />
          </TabsTrigger>
        ))}
      </TabsList>
      {configs.data.map((config) => (
        <TabsContent key={config.agent_kind} value={config.agent_kind} className="pt-3">
          <AgentPanel
            config={config}
            // The catalog is one list for every CLI; each tab takes its own.
            models={models.filter((model) => model.agent_kind === config.agent_kind)}
            modelsPending={modelsPending}
            onEdit={() => onEdit(config)}
          />
        </TabsContent>
      ))}
    </Tabs>
  )
}

function NoAgents() {
  return (
    <EmptyState
      title="No agents"
      description="An agent config is the flag list a coding-agent CLI is launched with. There is none to create — the daemon ships one per agent kind — so an empty list means it reported none; the flags can also be set with ariadne agent update."
    />
  )
}

function AgentPanel({
  config,
  models,
  modelsPending,
  onEdit,
}: {
  config: AgentConfigDto
  models: ModelDto[]
  modelsPending: boolean
  onEdit: () => void
}) {
  const label = AGENT_KIND_LABELS[config.agent_kind] ?? config.agent_kind
  const customized = !sameFlags(config.extra_flags, config.default_flags)

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-muted-foreground text-sm">Extra flags</span>
        {config.extra_flags.length > 0 ? (
          <div className="flex flex-wrap gap-1.5">
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
        <Button variant="ghost" size="sm" onClick={onEdit} aria-label={`Edit ${label} flags`}>
          <PencilIcon />
          Edit flags
        </Button>
      </div>

      <ModelTable models={models} isPending={modelsPending} />
    </div>
  )
}
