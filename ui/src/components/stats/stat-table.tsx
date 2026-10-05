/**
 * A compact comparison table of a section: one row per thing compared, the
 * columns the caller gives. A numeric column is right-aligned in tabular
 * figures, so its values line up digit for digit down the column.
 */

import type { ReactNode } from "react"

import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { cn } from "@/lib/format"
import { StatExplain } from "./stat-explain"

export function StatTable<T>({
  rows,
  rowKey,
  columns,
  caption,
}: {
  rows: T[]
  rowKey: (row: T) => string
  columns: {
    header: string
    numeric?: boolean
    /** What this column means, in one short plain sentence. */
    explain: string
    render: (row: T) => ReactNode
  }[]
  /** What the table compares, for a screen reader. */
  caption: string
}) {
  return (
    <Table className="text-xs">
      <caption className="sr-only">{caption}</caption>
      <TableHeader>
        <TableRow>
          {columns.map((column) => (
            <TableHead key={column.header} className={cn("h-8", column.numeric && "text-right")}>
              <StatExplain explain={column.explain}>{column.header}</StatExplain>
            </TableHead>
          ))}
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row) => (
          <TableRow key={rowKey(row)}>
            {columns.map((column) => (
              <TableCell
                key={column.header}
                className={cn("py-1.5", column.numeric && "text-right tabular-nums")}
              >
                {column.render(row)}
              </TableCell>
            ))}
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
