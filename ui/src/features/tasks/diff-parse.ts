/**
 * A unified diff, split into the pieces the viewer needs.
 *
 * `GET /v1/tasks/{id}/diff` returns exactly what `git diff base...branch`
 * prints, so this parser handles what git emits: `diff --git` file headers,
 * the extended header lines (modes, renames, similarity), `@@` hunks, and
 * binary files that have no hunks at all.
 *
 * Anything it cannot make sense of stays available verbatim — every file keeps
 * its own slice of the input, and the viewer can always fall back to the raw
 * text.
 */

export type DiffLineKind = "context" | "add" | "del"

export interface DiffLine {
  kind: DiffLineKind
  /** The line without its `+`/`-`/space marker. */
  text: string
}

export interface DiffHunk {
  /** The `@@ -1,3 +1,4 @@` line, verbatim. */
  header: string
  /** The section heading git appends after the second `@@`, when it found one. */
  heading: string
  oldStart: number
  newStart: number
  lines: DiffLine[]
}

export type DiffChangeKind = "added" | "deleted" | "renamed" | "modified"

export interface DiffFile {
  /** Stable key for React; paths alone are not unique across a rename pair. */
  id: string
  /** What to show in the file header: `old → new` for a rename. */
  path: string
  /** `null` when the file was added. */
  oldPath: string | null
  /** `null` when the file was deleted. */
  newPath: string | null
  change: DiffChangeKind
  binary: boolean
  additions: number
  deletions: number
  hunks: DiffHunk[]
  /** Header lines worth showing verbatim (mode changes, binary notices, …). */
  notes: string[]
  /** This file's slice of the diff, unmodified. */
  raw: string
}

export interface ParsedDiff {
  files: DiffFile[]
  additions: number
  deletions: number
  /** Text that appeared before the first `diff --git`, if any. */
  preamble: string
}

const HUNK_HEADER = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@ ?(.*)$/

/** Header lines that say something the file header should repeat. */
const NOTABLE_HEADERS = [
  "old mode ",
  "new mode ",
  "deleted file mode ",
  "new file mode ",
  "similarity index ",
  "dissimilarity index ",
  "copy from ",
  "copy to ",
  "GIT binary patch",
]

export function parseUnifiedDiff(text: string): ParsedDiff {
  const files: DiffFile[] = []
  if (text.trim().length === 0) return { files, additions: 0, deletions: 0, preamble: "" }

  const lines = text.split("\n")
  // The trailing newline of the diff is not a line of it.
  if (lines.at(-1) === "") lines.pop()
  // Where each `diff --git` starts, so a file's raw slice is exact.
  const starts: number[] = []
  lines.forEach((line, index) => {
    if (line.startsWith("diff --git ")) starts.push(index)
  })

  const preamble = starts.length > 0 ? lines.slice(0, starts[0]).join("\n") : text
  starts.forEach((start, index) => {
    const end = index + 1 < starts.length ? starts[index + 1] : lines.length
    files.push(parseFile(lines.slice(start, end), index))
  })

  return {
    files,
    additions: files.reduce((total, file) => total + file.additions, 0),
    deletions: files.reduce((total, file) => total + file.deletions, 0),
    preamble: preamble.trim().length > 0 ? preamble : "",
  }
}

function parseFile(lines: string[], index: number): DiffFile {
  const [oldGuess, newGuess] = pathsFromGitHeader(lines[0] ?? "")
  let oldPath: string | null = oldGuess
  let newPath: string | null = newGuess
  let renamed = false
  let deletedFile = false
  let addedFile = false
  let binary = false
  const notes: string[] = []
  const hunks: DiffHunk[] = []
  let hunk: DiffHunk | null = null

  for (const line of lines.slice(1)) {
    if (hunk === null) {
      const match = HUNK_HEADER.exec(line)
      if (match) {
        hunk = {
          header: line,
          heading: match[5] ?? "",
          oldStart: Number(match[1]),
          newStart: Number(match[3]),
          lines: [],
        }
        hunks.push(hunk)
        continue
      }
      if (line.startsWith("rename from ")) {
        renamed = true
        oldPath = line.slice("rename from ".length)
        continue
      }
      if (line.startsWith("rename to ")) {
        renamed = true
        newPath = line.slice("rename to ".length)
        continue
      }
      if (line.startsWith("--- ")) {
        oldPath = stripPathPrefix(line.slice(4))
        if (oldPath === null) addedFile = true
        continue
      }
      if (line.startsWith("+++ ")) {
        newPath = stripPathPrefix(line.slice(4))
        if (newPath === null) deletedFile = true
        continue
      }
      if (line.startsWith("new file mode ")) addedFile = true
      if (line.startsWith("deleted file mode ")) deletedFile = true
      if (line.startsWith("Binary files ") || line === "GIT binary patch") {
        binary = true
        notes.push(line)
        continue
      }
      if (NOTABLE_HEADERS.some((prefix) => line.startsWith(prefix))) notes.push(line)
      continue
    }

    const next = HUNK_HEADER.exec(line)
    if (next) {
      hunk = {
        header: line,
        heading: next[5] ?? "",
        oldStart: Number(next[1]),
        newStart: Number(next[3]),
        lines: [],
      }
      hunks.push(hunk)
      continue
    }
    // `\ No newline at end of file` annotates the line before it; the viewer
    // has nowhere useful to put it, and dropping it keeps the documents honest.
    if (line.startsWith("\\")) continue
    if (line.startsWith("+")) hunk.lines.push({ kind: "add", text: line.slice(1) })
    else if (line.startsWith("-")) hunk.lines.push({ kind: "del", text: line.slice(1) })
    else if (line.startsWith(" ") || line.length === 0) {
      hunk.lines.push({ kind: "context", text: line.slice(1) })
    } else {
      // Not part of the hunk body: git is done with this file's content.
      hunk = null
      if (line.startsWith("Binary files ")) {
        binary = true
        notes.push(line)
      }
    }
  }

  const additions = countLines(hunks, "add")
  const deletions = countLines(hunks, "del")
  const change: DiffChangeKind = renamed
    ? "renamed"
    : addedFile
      ? "added"
      : deletedFile
        ? "deleted"
        : "modified"

  return {
    id: `${index}:${newPath ?? oldPath ?? "unknown"}`,
    path: renamed && oldPath && newPath ? `${oldPath} → ${newPath}` : (newPath ?? oldPath ?? "?"),
    oldPath: addedFile ? null : oldPath,
    newPath: deletedFile ? null : newPath,
    change,
    binary,
    additions,
    deletions,
    hunks,
    notes,
    raw: lines.join("\n"),
  }
}

function countLines(hunks: DiffHunk[], kind: DiffLineKind): number {
  return hunks.reduce(
    (total, hunk) => total + hunk.lines.filter((line) => line.kind === kind).length,
    0,
  )
}

/**
 * `diff --git a/x b/x`. Paths with spaces make this genuinely ambiguous, so it
 * is only a first guess: the `---`/`+++` lines that follow overwrite it.
 */
function pathsFromGitHeader(line: string): [string | null, string | null] {
  const rest = line.slice("diff --git ".length)
  const half = Math.floor(rest.length / 2)
  // The two halves are the same path in the common case, which makes the split
  // point unambiguous even when the path contains a space.
  if (rest.length % 2 === 1 && rest[half] === " ") {
    const left = stripPathPrefix(rest.slice(0, half))
    const right = stripPathPrefix(rest.slice(half + 1))
    if (left !== null && left === right) return [left, right]
  }
  const space = rest.indexOf(" ")
  if (space === -1) return [null, null]
  return [stripPathPrefix(rest.slice(0, space)), stripPathPrefix(rest.slice(space + 1))]
}

/** Drop git's `a/` / `b/` prefix; `/dev/null` means the file is not on that side. */
function stripPathPrefix(path: string): string | null {
  const trimmed = unquote(path.trim())
  if (trimmed === "/dev/null") return null
  if (trimmed.startsWith("a/") || trimmed.startsWith("b/")) return trimmed.slice(2)
  return trimmed
}

/** git quotes paths with unusual characters, C-style. */
function unquote(path: string): string {
  if (!path.startsWith('"') || !path.endsWith('"') || path.length < 2) return path
  try {
    return JSON.parse(path) as string
  } catch {
    return path.slice(1, -1)
  }
}

/**
 * The two documents a unified merge view compares, built from a file's hunks.
 *
 * Only hunk content is in the diff, so the documents are the hunks stitched
 * together rather than the whole file — which is why the line numbers cannot
 * be inferred from the document and are carried alongside it.
 */
export interface DiffDocuments {
  /** New-side content: what the editor shows. */
  doc: string
  /** Old-side content: what the merge view diffs against. */
  original: string
  /** For each line of `doc`, its line number in the new file. */
  lineNumbers: number[]
  /** Where each hunk's header goes: the `doc` line index it sits above. */
  hunkStarts: { line: number; header: string; heading: string }[]
  /** Total lines of `doc`, so callers can bail out on very large files. */
  lineCount: number
}

export function buildDiffDocuments(file: DiffFile): DiffDocuments {
  const newLines: string[] = []
  const oldLines: string[] = []
  const lineNumbers: number[] = []
  const hunkStarts: DiffDocuments["hunkStarts"] = []

  for (const hunk of file.hunks) {
    hunkStarts.push({ line: newLines.length, header: hunk.header, heading: hunk.heading })
    let newLine = hunk.newStart
    for (const line of hunk.lines) {
      if (line.kind !== "add") oldLines.push(line.text)
      if (line.kind !== "del") {
        newLines.push(line.text)
        lineNumbers.push(newLine)
        newLine += 1
      }
    }
  }

  return {
    doc: newLines.join("\n"),
    original: oldLines.join("\n"),
    lineNumbers,
    hunkStarts,
    lineCount: newLines.length,
  }
}
