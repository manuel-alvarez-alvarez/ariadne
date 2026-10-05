/**
 * The Stats screen: what the work did, read off the daemon's stats ledger,
 * one section per question a user asks of it.
 *
 * The header holds the two filters every stat family takes — how far back,
 * and which repository — and the screen hands them to each section as one
 * `{ since, repo }`. Both live in the URL (`?since=7d&repo=…`), so a reload or
 * a link keeps what the reader narrowed the screen to. Each family is a
 * section of its own under `components/stats/`.
 */

import { useQuery } from "@tanstack/react-query"
import { useMemo } from "react"
import { useSearchParams } from "react-router-dom"

import type { StatsFilter } from "@/api"
import { PageHeader } from "@/components/page-header"
import { AttentionSection } from "@/components/stats/attention-section"
import { ModelsSection } from "@/components/stats/models-section"
import { SpendSection } from "@/components/stats/spend-section"
import { TimeSection } from "@/components/stats/time-section"
import { ToolsSection } from "@/components/stats/tools-section"
import { WorkSection } from "@/components/stats/work-section"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { repositoriesQueryOptions } from "@/features/repositories/queries"
import { folderName, shortId } from "@/lib/format"

/** No filter, in a Select: the value an absent param stands for. */
const ALL = "all"

/** The spans the `since` selector offers, in the daemon's own spelling. */
const SPANS = [
  { value: ALL, label: "All time" },
  { value: "24h", label: "Last 24 hours" },
  { value: "7d", label: "Last 7 days" },
  { value: "30d", label: "Last 30 days" },
]

export function StatsPage() {
  const [search, setSearch] = useSearchParams()
  const since = search.get("since") ?? undefined
  const repo = search.get("repo") ?? undefined
  const filter: StatsFilter = useMemo(() => ({ since, repo }), [since, repo])

  const repositories = useQuery(repositoriesQueryOptions())
  const repositoryItems = useMemo(
    () => [
      { value: ALL, label: "All repositories" },
      ...(repositories.data ?? []).map((repository) => ({
        value: repository.id,
        label: folderName(repository.path),
      })),
    ],
    [repositories.data],
  )
  const repoLabel =
    repositoryItems.find((item) => item.value === (repo ?? ALL))?.label ?? shortId(repo ?? "")

  function setParam(name: "since" | "repo", value: string | null) {
    const next = new URLSearchParams(search)
    if (value === null || value === ALL) next.delete(name)
    else next.set(name, value)
    setSearch(next, { replace: true })
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Stats"
        description="What the work did, from what the daemon recorded as it happened."
        actions={
          <>
            <Select value={since ?? ALL} onValueChange={(v) => setParam("since", v)} items={SPANS}>
              <SelectTrigger aria-label="Since" className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SPANS.map((span) => (
                  <SelectItem key={span.value} value={span.value}>
                    {span.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select
              value={repo ?? ALL}
              onValueChange={(v) => setParam("repo", v)}
              items={repositoryItems}
            >
              <SelectTrigger aria-label="Repository" className="w-48">
                <SelectValue>{repoLabel}</SelectValue>
              </SelectTrigger>
              <SelectContent alignItemWithTrigger={false} className="w-80">
                {repositoryItems.map((item) => (
                  <SelectItem key={item.value} value={item.value}>
                    {item.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </>
        }
      />
      <div className="flex flex-col gap-6">
        <ModelsSection filter={filter} />
        <WorkSection filter={filter} />
        <TimeSection filter={filter} />
        <SpendSection filter={filter} />
        <AttentionSection filter={filter} />
        <ToolsSection filter={filter} />
      </div>
    </div>
  )
}
