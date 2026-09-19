/**
 * The knowledge screen (022): what the knowledge base holds for every
 * registered repository, at `#/knowledge` — parity with `ariadne knowledge`
 * (014's parity rule, spec 015).
 *
 * A repository picker and a ref picker lead it, and the tabs below read what
 * they picked. All three live in the URL — `?repository=`, `?ref=`, `?tab=` —
 * so a reload or a link lands on the same view. Without them the screen reads
 * the first repository, at its base branch, on the Overview tab.
 */

import { useQuery } from "@tanstack/react-query"
import { Link, useSearchParams } from "react-router-dom"

import { ErrorState } from "@/components/error-state"
import { PageHeader } from "@/components/page-header"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { NoRepositories } from "@/features/repositories/no-repositories"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { folderName } from "@/lib/format"
import { paths } from "@/routes/paths"

import { KNOWLEDGE_TABS } from "./knowledge-tabs"
import { knowledgeStatusQueryOptions } from "./queries"

const DESCRIPTION =
  "What the knowledge base has read from each repository, and how the repositories use each other."

export function KnowledgeScreen() {
  const [search, setSearch] = useSearchParams()
  const repositories = useQuery(repositoriesQueryOptions())

  const header = <PageHeader title="Knowledge" description={DESCRIPTION} />

  if (repositories.isPending) {
    return (
      <div className="flex flex-col gap-4">
        {header}
        <Skeleton className="h-8 w-96" />
        <Skeleton className="h-64 w-full" />
      </div>
    )
  }
  if (repositories.isError) {
    return (
      <div className="flex flex-col gap-4">
        {header}
        <ErrorState
          title="Could not load the repositories"
          error={repositories.error}
          onRetry={() => void repositories.refetch()}
          showIcon
        />
      </div>
    )
  }

  const list = repositories.data
  const repository = list.find((row) => row.id === search.get("repository")) ?? list[0]
  if (!repository) {
    return (
      <div className="flex flex-col gap-4">
        {header}
        <NoRepositories
          action={
            <Button variant="outline" size="sm" render={<Link to={paths.repositories()} />}>
              Go to repositories
            </Button>
          }
        />
      </div>
    )
  }

  const gitRef = search.get("ref") ?? repository.base_branch
  const symbolName = search.get("symbol") ?? ""
  const tab = KNOWLEDGE_TABS.find((entry) => entry.id === search.get("tab")) ?? KNOWLEDGE_TABS[0]

  /** Writes the picks into the URL, leaving every other param where it was. */
  const pick = (changes: Record<string, string | null>) =>
    setSearch(
      (current) => {
        const next = new URLSearchParams(current)
        for (const [key, value] of Object.entries(changes)) {
          if (value === null) next.delete(key)
          else next.set(key, value)
        }
        return next
      },
      { replace: true },
    )
  // A ref belongs to one repository: another repository starts at its own base.
  const pickRepository = (id: string) => pick({ repository: id, ref: null })
  const pickSymbol = (name: string, repositoryId: string, ref: string) =>
    pick({ symbol: name, repository: repositoryId, ref })

  const repositoryOptions = list.map((row) => ({ value: row.id, label: folderName(row.path) }))

  return (
    <div className="flex flex-col gap-4">
      {header}

      <div className="flex flex-wrap items-center gap-2">
        <Select
          value={repository.id}
          onValueChange={(value) => value && pickRepository(value)}
          items={repositoryOptions}
        >
          <SelectTrigger aria-label="Repository" className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {repositoryOptions.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <RefPicker
          repositoryId={repository.id}
          baseRef={repository.base_branch}
          value={gitRef}
          onPick={(ref) => pick({ ref })}
        />
      </div>

      <Tabs value={tab?.id} onValueChange={(value) => pick({ tab: String(value) })}>
        <TabsList>
          {KNOWLEDGE_TABS.map((entry) => (
            <TabsTrigger key={entry.id} value={entry.id}>
              {entry.label}
            </TabsTrigger>
          ))}
        </TabsList>
        {tab ? (
          <TabsContent value={tab.id} className="pt-3">
            {tab.render({
              repositories: list,
              repository,
              gitRef,
              symbolName,
              pickRepository,
              pickSymbol,
            })}
          </TabsContent>
        ) : null}
      </Tabs>
    </div>
  )
}

/**
 * The refs the repository has indexed, from its status, and its base branch
 * even before that is indexed: the default has to be one of the choices.
 */
function RefPicker({
  repositoryId,
  baseRef,
  value,
  onPick,
}: {
  repositoryId: string
  baseRef: string
  value: string
  onPick: (ref: string) => void
}) {
  const status = useQuery(knowledgeStatusQueryOptions(repositoryId))
  const refs = [
    ...new Set([baseRef, value, ...(status.data?.refs.map((ref) => ref.git_ref) ?? [])]),
  ]
  const options = refs.map((ref) => ({ value: ref, label: ref }))

  return (
    <Select value={value} onValueChange={(ref) => ref && onPick(ref)} items={options}>
      <SelectTrigger aria-label="Ref" className="w-56 font-mono">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((option) => (
          <SelectItem key={option.value} value={option.value} className="font-mono">
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}
