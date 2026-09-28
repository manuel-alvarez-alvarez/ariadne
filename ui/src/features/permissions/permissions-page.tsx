/**
 * The Permissions screen: what a repository's agents may do without asking
 * again, in two tabs.
 *
 * Learned opens first-class, ahead of AI, because it is where every mode —
 * `learn` and `ai` alike — leaves what it decided; AI, unchanged from before
 * this screen had a second tab, holds the one settings row behind the model
 * itself (see `ai-card.tsx`). The tab lives in the URL, like every other panel
 * and screen state in the app, so a link can point at either and a reload
 * stays put.
 *
 * A screen of its own, rather than a section of Repositories, because both are
 * the daemon's: the AI model is one install shared by every repository that
 * picks `ai`, and a learned approval outlives the session or the console pick
 * that made it.
 */

import { useQuery } from "@tanstack/react-query"
import { useSearchParams } from "react-router-dom"

import { ApiError } from "@/api"
import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

import { AiCard } from "./ai-card"
import { LearnedPermissionsTab } from "./learned-tab"
import { aiPermissionsStatusQueryOptions } from "./queries"

const TABS = ["learned", "ai"] as const
type Tab = (typeof TABS)[number]

/** Where the screen opens when the URL does not say: Learned, where every mode leaves what it decided. */
const DEFAULT_TAB: Tab = "learned"

export function PermissionsPage() {
  const [search, setSearch] = useSearchParams()
  const tab = TABS.find((value) => value === search.get("tab")) ?? DEFAULT_TAB

  function setTab(next: Tab) {
    const params = new URLSearchParams(search)
    params.set("tab", next)
    setSearch(params, { replace: true })
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Permissions"
        description="What a repository's agents may do without asking again — approvals learned from a console pick or added by hand, and the AI model that answers the rest."
      />

      <Tabs value={tab} onValueChange={(value) => setTab(value as Tab)}>
        <TabsList>
          <TabsTrigger value="learned">Learned</TabsTrigger>
          <TabsTrigger value="ai">AI</TabsTrigger>
        </TabsList>
        <TabsContent value="learned" className="pt-3">
          <LearnedPermissionsTab />
        </TabsContent>
        <TabsContent value="ai" className="pt-3">
          <AiTab />
        </TabsContent>
      </Tabs>
    </div>
  )
}

function AiTab() {
  const status = useQuery(aiPermissionsStatusQueryOptions())

  return (
    <div className="flex flex-col gap-4">
      {status.isPending ? <Skeleton className="h-72 rounded-xl" /> : null}

      {status.isError ? (
        <ErrorState
          title="Could not load the model's settings"
          error={status.error}
          description={networkHint(status.error)}
          onRetry={() => void status.refetch()}
        />
      ) : null}

      {status.data ? <AiCard status={status.data} /> : null}
    </div>
  )
}

/** A daemon that never answered has nothing to say about why. */
function networkHint(error: unknown): string | undefined {
  return ApiError.is(error) && error.isNetworkError
    ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
    : undefined
}
