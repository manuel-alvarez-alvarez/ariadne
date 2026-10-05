/**
 * The Work section of the Stats screen: what got done? It reads
 * `GET /v1/stats/work` under the screen's filter — tiles of the headline
 * figures, and a chart of tasks per bucket stacked by how they ended.
 */

import { queryOptions, useQuery } from "@tanstack/react-query"

import { api, qk, type StatsFilter, unwrap, type WorkBucketDto, type WorkStatsDto } from "@/api"
import { formatDuration } from "@/lib/format"
import { WORK_CONFIG } from "./chart-configs"
import { StatSection } from "./stat-section"
import { StatTile, StatTiles } from "./stat-tiles"
import { StatTimeChart } from "./stat-time-chart"

/** `GET /v1/stats/work`, narrowed by the screen's filter. */
export function workStatsQueryOptions(filter: StatsFilter) {
  return queryOptions({
    queryKey: qk.stats.work(filter),
    queryFn: (): Promise<WorkStatsDto> =>
      unwrap(api().GET("/v1/stats/work", { params: { query: filter } })),
  })
}

/** A fraction as a tile reads it: `73.3%`. */
function percent(fraction: number): string {
  return `${(fraction * 100).toFixed(1)}%`
}

/** A bucket as [`StatTimeChart`] draws it: its start under the `bucket` key
 * every row of its axis carries. */
function chartRow(bucket: WorkBucketDto) {
  return {
    bucket: bucket.start,
    tasks_finished: bucket.tasks_finished,
    tasks_failed: bucket.tasks_failed,
    tasks_cancelled: bucket.tasks_cancelled,
    goals_completed: bucket.goals_completed,
    landed: bucket.landed,
  }
}

export function WorkSection({ filter }: { filter: StatsFilter }) {
  const stats = useQuery(workStatsQueryOptions(filter))
  return (
    <StatSection
      title="Work"
      description="What got done?"
      query={stats}
      isEmpty={(data) => (data.buckets?.length ?? 0) === 0}
      empty="Nothing got done in this span."
    >
      {(data) => (
        <div className="flex flex-1 flex-col gap-4">
          <StatTiles>
            <StatTile label="Tasks finished" value={data.totals.tasks_finished} />
            <StatTile label="Goals completed" value={data.totals.goals_completed} />
            <StatTile label="Changes landed" value={data.totals.landed} />
            <StatTile label="Finish rate" value={percent(data.totals.finish_rate)} />
            <StatTile
              label="Median goal lead time"
              value={formatDuration(data.totals.median_goal_lead_time_secs)}
            />
          </StatTiles>
          <StatTimeChart
            data={data.buckets.map(chartRow)}
            config={WORK_CONFIG}
            keys={["tasks_finished", "tasks_failed", "tasks_cancelled"]}
            extra={["goals_completed", "landed"]}
            caption="Tasks per bucket, by how they ended"
          />
        </div>
      )}
    </StatSection>
  )
}
