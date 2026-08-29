/**
 * The three list screens — profiles, repositories, agents — as one table.
 *
 * Each was spelling the same forty lines out for itself: the bordered frame,
 * the header row, three shimmering rows while the list loads, the empty state
 * stretched across every column, and the alert when the daemon does not answer
 * — down to the same sentence about checking the URL in settings. They had not
 * drifted; only the column headings and the alert's title ever differed, and
 * both are parameters here. Three identical copies is simply three places to
 * fix the next thing that is wrong with any of them.
 *
 * The sessions table is deliberately not one of these: its error renders
 * *above* its rows rather than instead of them, so a list that half-loaded
 * still shows what it has.
 *
 * What stays with the screen is the part that is its own: which columns, what
 * a row looks like, and what to say when there are none.
 */

import { Fragment, type ReactNode } from "react"

import { ApiError } from "@/api"
import { ErrorState } from "@/components/error-state"
import { ScrollableTable } from "@/components/scroll-edge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

/** Rows drawn while the list loads. Enough to read as a table, not as a page. */
const PLACEHOLDER_ROWS = [0, 1, 2]

/** One column: its heading, and how wide the cell holding it may get. */
interface DataColumn {
  /** Absent for the actions column, which is labelled for screen readers only. */
  header?: ReactNode
  className?: string
}

/** What every list screen reads off its query. */
interface ListQuery<T> {
  data?: T[]
  isPending: boolean
  isError: boolean
  error: unknown
  refetch: () => unknown
}

export function DataTable<T>({
  query,
  errorTitle,
  columns,
  empty,
  rowKey,
  renderRow,
}: {
  query: ListQuery<T>
  /** "Could not load profiles" — the alert's heading when the read fails. */
  errorTitle: string
  columns: DataColumn[]
  /** What the table holds instead of rows when the daemon answered with none. */
  empty: ReactNode
  rowKey: (row: T) => string
  renderRow: (row: T) => ReactNode
}) {
  if (query.isError) {
    return (
      <ErrorState
        title={errorTitle}
        error={query.error}
        // A daemon that never answered has nothing to say about why.
        description={
          ApiError.is(query.error) && query.error.isNetworkError
            ? "The daemon is not answering. Check the URL in settings and that it is listening on TCP."
            : undefined
        }
        onRetry={() => void query.refetch()}
      />
    )
  }

  return (
    // A screen this narrow for its columns scrolls sideways, and says so:
    // macOS draws no scrollbar until something moves, so the fade at the edge
    // is the only sign that a column is cut short.
    <ScrollableTable className="rounded-xl border">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          {columns.map((column, index) => (
            <TableHead
              // Columns are a fixed list per screen; a heading may repeat.
              // biome-ignore lint/suspicious/noArrayIndexKey: the position is the identity
              key={index}
              className={column.className}
            >
              {column.header ?? <span className="sr-only">Actions</span>}
            </TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {query.isPending ? (
          PLACEHOLDER_ROWS.map((row) => (
            <TableRow key={row} className="hover:bg-transparent">
              {columns.map((_column, index) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: placeholder cells have no identity
                <TableCell key={index}>
                  <Skeleton className="h-4 w-full" />
                </TableCell>
              ))}
            </TableRow>
          ))
        ) : query.data?.length ? (
          query.data.map((row) => <Fragment key={rowKey(row)}>{renderRow(row)}</Fragment>)
        ) : (
          <TableRow className="hover:bg-transparent">
            {/* The table's own frame is the box around the empty state. */}
            <TableCell colSpan={columns.length} className="p-0">
              {empty}
            </TableCell>
          </TableRow>
        )}
      </TableBody>
    </ScrollableTable>
  )
}

/** One of the ghost icon buttons at the end of a row. */
export function RowAction({
  icon,
  label,
  onClick,
}: {
  icon: ReactNode
  /** Both the accessible name and the tooltip: "Edit rust-engineer". */
  label: string
  onClick: () => void
}) {
  return (
    <Button variant="ghost" size="icon-sm" aria-label={label} title={label} onClick={onClick}>
      {icon}
    </Button>
  )
}
