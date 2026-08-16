/**
 * The task branch against its base — the `task diff` equivalent.
 *
 * The daemon answers `409` while the branch does not exist yet (nothing has
 * been committed, or the task never started), which is a normal state for most
 * of a task's life rather than an error, so it gets its own empty state.
 */

import { useQuery } from "@tanstack/react-query"
import {
  ChevronDownIcon,
  ChevronRightIcon,
  FileDiffIcon,
  FileIcon,
  FileMinusIcon,
  FilePlusIcon,
  RefreshCwIcon,
} from "lucide-react"
import { useMemo, useState } from "react"

import { ApiError } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { DiffEditor, LARGE_FILE_LINES } from "./diff-editor"
import { type DiffFile, parseUnifiedDiff } from "./diff-parse"
import { taskDiffQueryOptions } from "./queries"

const CHANGE_META: Record<DiffFile["change"], { label: string; icon: typeof FileIcon }> = {
  added: { label: "added", icon: FilePlusIcon },
  deleted: { label: "deleted", icon: FileMinusIcon },
  renamed: { label: "renamed", icon: FileDiffIcon },
  modified: { label: "modified", icon: FileIcon },
}

export function TaskDiff({ taskId }: { taskId: string }) {
  const diff = useQuery(taskDiffQueryOptions(taskId))
  const [raw, setRaw] = useState(false)
  const parsed = useMemo(() => parseUnifiedDiff(diff.data ?? ""), [diff.data])

  if (diff.isPending) {
    return (
      <div className="space-y-2">
        <Skeleton className="h-8 w-full" />
        <Skeleton className="h-40 w-full" />
      </div>
    )
  }

  if (diff.error) return <DiffError error={diff.error} onRetry={() => void diff.refetch()} />

  const empty = (diff.data ?? "").trim().length === 0

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm text-muted-foreground">
          {empty
            ? "No changes"
            : `${parsed.files.length} ${parsed.files.length === 1 ? "file" : "files"} changed`}
        </span>
        {!empty && <DiffStat additions={parsed.additions} deletions={parsed.deletions} />}
        <div className="ml-auto flex items-center gap-1">
          <Button
            variant={raw ? "secondary" : "ghost"}
            size="sm"
            onClick={() => setRaw((current) => !current)}
            disabled={empty}
          >
            Raw
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void diff.refetch()}
            disabled={diff.isFetching}
          >
            <RefreshCwIcon className={cn(diff.isFetching && "animate-spin")} />
            Refresh
          </Button>
        </div>
      </div>

      {empty ? (
        <EmptyState
          emphasis="quiet"
          title="The branch exists but has no changes against its base yet."
        />
      ) : raw || parsed.files.length === 0 ? (
        <RawDiff text={diff.data ?? ""} />
      ) : (
        <div className="space-y-3">
          {parsed.preamble && <RawDiff text={parsed.preamble} />}
          {parsed.files.map((file) => (
            <DiffFileSection key={file.id} file={file} />
          ))}
        </div>
      )}
    </div>
  )
}

function DiffFileSection({ file }: { file: DiffFile }) {
  const lineCount = useMemo(
    () => file.hunks.reduce((total, hunk) => total + hunk.lines.length, 0),
    [file],
  )
  const huge = lineCount > LARGE_FILE_LINES
  const [open, setOpen] = useState(!huge)
  const [showRaw, setShowRaw] = useState(false)
  const { label, icon: Icon } = CHANGE_META[file.change]

  return (
    <section className="overflow-hidden rounded-lg border">
      <header className="flex flex-wrap items-center gap-2 border-b bg-muted/40 px-2 py-1.5">
        <Button
          variant="ghost"
          size="icon-xs"
          onClick={() => setOpen((current) => !current)}
          aria-expanded={open}
          aria-label={open ? `Collapse ${file.path}` : `Expand ${file.path}`}
        >
          {open ? <ChevronDownIcon /> : <ChevronRightIcon />}
        </Button>
        <Icon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 truncate font-mono text-xs" title={file.path}>
          {file.path}
        </span>
        <Badge variant="outline" className="shrink-0">
          {label}
        </Badge>
        {!file.binary && <DiffStat additions={file.additions} deletions={file.deletions} />}
      </header>

      {open && (
        <div className="min-w-0">
          {file.notes.length > 0 && (
            <p className="border-b bg-muted/20 px-3 py-1.5 font-mono text-xs text-muted-foreground">
              {file.notes.join(" · ")}
            </p>
          )}
          {file.binary ? (
            <p className="px-3 py-2 text-sm text-muted-foreground">
              Binary file — no textual diff.
            </p>
          ) : file.hunks.length === 0 ? (
            <p className="px-3 py-2 text-sm text-muted-foreground">
              No content changes (metadata only).
            </p>
          ) : huge && !showRaw ? (
            <div className="flex flex-wrap items-center gap-2 px-3 py-2 text-sm text-muted-foreground">
              {lineCount.toLocaleString()} changed lines — too large to render.
              <Button variant="outline" size="xs" onClick={() => setShowRaw(true)}>
                Show raw
              </Button>
            </div>
          ) : huge ? (
            <RawDiff text={file.raw} bare />
          ) : (
            <DiffEditor file={file} />
          )}
        </div>
      )}
    </section>
  )
}

function DiffStat({ additions, deletions }: { additions: number; deletions: number }) {
  return (
    <span className="shrink-0 font-mono text-xs">
      <span className="text-emerald-600 dark:text-emerald-400">+{additions}</span>{" "}
      <span className="text-red-600 dark:text-red-400">−{deletions}</span>
    </span>
  )
}

/** The fallback: the diff exactly as the daemon sent it. */
function RawDiff({ text, bare = false }: { text: string; bare?: boolean }) {
  return (
    <pre
      className={cn(
        "overflow-x-auto px-3 py-2 font-mono text-xs leading-relaxed",
        !bare && "rounded-lg border bg-muted/20",
      )}
    >
      {text}
    </pre>
  )
}

function DiffError({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  // 409 is the daemon saying "there is nothing to diff yet", not a failure.
  if (ApiError.is(error) && error.status === 409) {
    return <EmptyState emphasis="quiet" title={error.message} />
  }
  return <ErrorState title="Could not load the diff" error={error} onRetry={onRetry} />
}
