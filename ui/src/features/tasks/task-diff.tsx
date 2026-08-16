/**
 * The task branch against its base — the `task diff` equivalent.
 *
 * The daemon answers `409` while the branch does not exist yet (nothing has
 * been committed, or the task never started), which is a normal state for most
 * of a task's life rather than an error, so it gets its own empty state.
 *
 * The panel it lives in is narrow, and a wide diff does not fit it, so the same
 * viewer — toolbar, file sections, raw fallback and all — can be lifted into a
 * near-fullscreen dialog. It is lifted rather than duplicated: one instance,
 * mounted in whichever of the two frames is showing.
 */

import { useQuery } from "@tanstack/react-query"
import {
  ChevronDownIcon,
  ChevronRightIcon,
  FileDiffIcon,
  FileIcon,
  FileMinusIcon,
  FilePlusIcon,
  Maximize2Icon,
  Minimize2Icon,
  RefreshCwIcon,
  WrapTextIcon,
} from "lucide-react"
import { useMemo, useState } from "react"

import { ApiError } from "@/api"
import { EmptyState } from "@/components/empty-state"
import { ErrorState } from "@/components/error-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { DiffEditor, LARGE_FILE_LINES } from "./diff-editor"
import { type DiffFile, type ParsedDiff, parseUnifiedDiff } from "./diff-parse"
import { useDiffWrap } from "./diff-prefs"
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
  const [expanded, setExpanded] = useState(false)
  // Which file sections the reader has folded open or away, by file id: the
  // viewer is remounted when it moves into the expanded view, and this is what
  // keeps that move from undoing their navigation.
  const [openFiles, setOpenFiles] = useState<Record<string, boolean>>({})
  const { wrap, setWrap } = useDiffWrap()
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

  const toolbar = (
    <DiffToolbar
      files={parsed.files.length}
      additions={parsed.additions}
      deletions={parsed.deletions}
      empty={empty}
      raw={raw}
      onRaw={() => setRaw((current) => !current)}
      wrap={wrap}
      onWrap={() => setWrap(!wrap)}
      expanded={expanded}
      onExpanded={() => setExpanded((current) => !current)}
      fetching={diff.isFetching}
      onRefresh={() => void diff.refetch()}
    />
  )

  const body = empty ? (
    <EmptyState
      emphasis="quiet"
      title="The branch exists but has no changes against its base yet."
    />
  ) : raw || parsed.files.length === 0 ? (
    <RawDiff text={diff.data ?? ""} wrap={wrap} />
  ) : (
    <DiffFiles
      parsed={parsed}
      wrap={wrap}
      openFiles={openFiles}
      onOpenFile={(fileId, open) => setOpenFiles((current) => ({ ...current, [fileId]: open }))}
    />
  )

  if (!expanded) {
    return (
      <div className="space-y-3">
        {toolbar}
        {body}
      </div>
    )
  }

  return (
    <>
      {/* The viewer itself is in the dialog; the tab keeps its height rather
          than collapsing behind it, and says where the diff went. */}
      <EmptyState
        emphasis="quiet"
        title="The diff is open in the expanded view."
        action={
          <Button variant="outline" size="sm" onClick={() => setExpanded(false)}>
            <Minimize2Icon />
            Back to the panel
          </Button>
        }
      />
      <Dialog open onOpenChange={(open) => open || setExpanded(false)}>
        {/* Near-fullscreen, and a two-row grid so only the diff scrolls: the
            toolbar — file navigation, Raw, wrap — stays put above it. */}
        <DialogContent
          showCloseButton={false}
          className="h-[calc(100dvh-2rem)] w-[calc(100vw-2rem)] max-w-[calc(100vw-2rem)] grid-rows-[auto_minmax(0,1fr)] gap-3 sm:max-w-[calc(100vw-2rem)]"
        >
          <DialogTitle className="sr-only">Diff of the task branch</DialogTitle>
          {toolbar}
          <div className="min-h-0 overflow-y-auto">{body}</div>
        </DialogContent>
      </Dialog>
    </>
  )
}

function DiffToolbar({
  files,
  additions,
  deletions,
  empty,
  raw,
  onRaw,
  wrap,
  onWrap,
  expanded,
  onExpanded,
  fetching,
  onRefresh,
}: {
  files: number
  additions: number
  deletions: number
  empty: boolean
  raw: boolean
  onRaw: () => void
  wrap: boolean
  onWrap: () => void
  expanded: boolean
  onExpanded: () => void
  fetching: boolean
  onRefresh: () => void
}) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="text-sm text-muted-foreground">
        {empty ? "No changes" : `${files} ${files === 1 ? "file" : "files"} changed`}
      </span>
      {!empty && <DiffStat additions={additions} deletions={deletions} />}
      <div className="ml-auto flex items-center gap-1">
        <Button
          variant={wrap ? "secondary" : "ghost"}
          size="sm"
          onClick={onWrap}
          aria-pressed={wrap}
          title={wrap ? "Stop wrapping long lines" : "Wrap long lines"}
        >
          <WrapTextIcon />
          Wrap
        </Button>
        <Button variant={raw ? "secondary" : "ghost"} size="sm" onClick={onRaw} disabled={empty}>
          Raw
        </Button>
        <Button variant="ghost" size="sm" onClick={onRefresh} disabled={fetching}>
          <RefreshCwIcon className={cn(fetching && "animate-spin")} />
          Refresh
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={onExpanded}
          disabled={empty}
          aria-pressed={expanded}
          aria-label={expanded ? "Collapse the diff back into the panel" : "Expand the diff"}
          title={expanded ? "Collapse the diff back into the panel" : "Expand the diff"}
        >
          {expanded ? <Minimize2Icon /> : <Maximize2Icon />}
        </Button>
      </div>
    </div>
  )
}

function DiffFiles({
  parsed,
  wrap,
  openFiles,
  onOpenFile,
}: {
  parsed: ParsedDiff
  wrap: boolean
  openFiles: Record<string, boolean>
  onOpenFile: (fileId: string, open: boolean) => void
}) {
  return (
    <div className="space-y-3">
      {parsed.preamble && <RawDiff text={parsed.preamble} wrap={wrap} />}
      {parsed.files.map((file) => (
        <DiffFileSection
          key={file.id}
          file={file}
          wrap={wrap}
          openOverride={openFiles[file.id]}
          onOpen={(open) => onOpenFile(file.id, open)}
        />
      ))}
    </div>
  )
}

function DiffFileSection({
  file,
  wrap,
  openOverride,
  onOpen,
}: {
  file: DiffFile
  wrap: boolean
  /** What the reader chose for this file, or `undefined` for the default. */
  openOverride: boolean | undefined
  onOpen: (open: boolean) => void
}) {
  const lineCount = useMemo(
    () => file.hunks.reduce((total, hunk) => total + hunk.lines.length, 0),
    [file],
  )
  const huge = lineCount > LARGE_FILE_LINES
  // Folded-away by default when it is too big to render, and after that
  // whatever the reader last said — which the viewer holds on their behalf so
  // it survives the move between the panel and the expanded view.
  const open = openOverride ?? !huge
  const [showRaw, setShowRaw] = useState(false)
  const { label, icon: Icon } = CHANGE_META[file.change]

  return (
    <section className="overflow-hidden rounded-lg border">
      <header className="flex flex-wrap items-center gap-2 border-b bg-muted/40 px-2 py-1.5">
        <Button
          variant="ghost"
          size="icon-xs"
          onClick={() => onOpen(!open)}
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
            <RawDiff text={file.raw} wrap={wrap} bare />
          ) : (
            <DiffEditor file={file} wrap={wrap} />
          )}
        </div>
      )}
    </section>
  )
}

function DiffStat({ additions, deletions }: { additions: number; deletions: number }) {
  return (
    <span className="shrink-0 font-mono text-xs">
      <span className="text-diff-add-fg">+{additions}</span>{" "}
      <span className="text-diff-remove-fg">−{deletions}</span>
    </span>
  )
}

/** The fallback: the diff exactly as the daemon sent it. */
function RawDiff({ text, wrap, bare = false }: { text: string; wrap: boolean; bare?: boolean }) {
  return (
    <pre
      className={cn(
        "px-3 py-2 font-mono text-xs leading-relaxed",
        // The wrap toggle means the same thing here as it does in the editor,
        // so the raw view follows it rather than always scrolling sideways.
        wrap ? "whitespace-pre-wrap break-words" : "overflow-x-auto",
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
