import { describe, expect, it } from "vitest"

import { buildDiffDocuments, type DiffFile, type ParsedDiff, parseUnifiedDiff } from "./diff-parse"

const MULTI_FILE = `diff --git a/src/lib.rs b/src/lib.rs
index 83db48f..bf269f4 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@
 //! crate docs
-pub fn old() {}
+pub fn renamed() {}
+pub fn added() {}

@@ -20,3 +21,3 @@ mod tests {
     #[test]
-    fn a() {}
+    fn b() {}
diff --git a/docs/new.md b/docs/new.md
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/docs/new.md
@@ -0,0 +1,2 @@
+# New
+body
diff --git a/old.txt b/old.txt
deleted file mode 100644
index 1234567..0000000
--- a/old.txt
+++ /dev/null
@@ -1,2 +0,0 @@
-gone
-also gone
`

/** Index into a parse result, failing loudly rather than with `undefined`. */
function fileAt(parsed: ParsedDiff, index: number): DiffFile {
  const file = parsed.files[index]
  if (!file) throw new Error(`the diff has no file at index ${index}`)
  return file
}

describe("parseUnifiedDiff", () => {
  it("returns nothing for an empty diff", () => {
    expect(parseUnifiedDiff("")).toEqual({ files: [], additions: 0, deletions: 0, preamble: "" })
  })

  it("splits a multi-file diff and counts each side", () => {
    const parsed = parseUnifiedDiff(MULTI_FILE)

    expect(parsed.files.map((file) => file.path)).toEqual(["src/lib.rs", "docs/new.md", "old.txt"])
    expect(parsed.files.map((file) => file.change)).toEqual(["modified", "added", "deleted"])
    expect(parsed.additions).toBe(5)
    expect(parsed.deletions).toBe(4)

    expect(fileAt(parsed, 0).hunks).toHaveLength(2)
    expect(fileAt(parsed, 0).hunks[1]?.heading).toBe("mod tests {")
    expect(fileAt(parsed, 1).oldPath).toBeNull()
    expect(fileAt(parsed, 2).newPath).toBeNull()
  })

  it("keeps each file's own slice of the input verbatim", () => {
    const file = fileAt(parseUnifiedDiff(MULTI_FILE), 1)

    expect(file.raw.split("\n")[0]).toBe("diff --git a/docs/new.md b/docs/new.md")
    expect(file.raw.endsWith("+body")).toBe(true)
  })

  it("recognises renames and mode changes", () => {
    const file = fileAt(
      parseUnifiedDiff(`diff --git a/a.txt b/b.txt
similarity index 92%
rename from a.txt
rename to b.txt
--- a/a.txt
+++ b/b.txt
@@ -1 +1 @@
-one
+two
`),
      0,
    )

    expect(file.change).toBe("renamed")
    expect(file.path).toBe("a.txt → b.txt")
    expect(file.notes).toEqual(["similarity index 92%"])
  })

  it("recognises binary files, which have no hunks", () => {
    const file = fileAt(
      parseUnifiedDiff(`diff --git a/logo.png b/logo.png
index 1234567..89abcde 100644
Binary files a/logo.png and b/logo.png differ
`),
      0,
    )

    expect(file.binary).toBe(true)
    expect(file.hunks).toEqual([])
    expect(file.notes).toEqual(["Binary files a/logo.png and b/logo.png differ"])
  })

  it("drops the no-newline marker rather than treating it as content", () => {
    const file = fileAt(
      parseUnifiedDiff(`diff --git a/a b/a
--- a/a
+++ b/a
@@ -1 +1 @@
-one
\\ No newline at end of file
+two
`),
      0,
    )

    expect(file.hunks[0]?.lines).toEqual([
      { kind: "del", text: "one" },
      { kind: "add", text: "two" },
    ])
  })

  it("handles paths containing spaces", () => {
    const file = fileAt(
      parseUnifiedDiff(`diff --git a/my dir/f.txt b/my dir/f.txt
--- a/my dir/f.txt
+++ b/my dir/f.txt
@@ -1 +1 @@
-a
+b
`),
      0,
    )

    expect(file.path).toBe("my dir/f.txt")
  })
})

describe("buildDiffDocuments", () => {
  it("stitches the hunks into an old and a new document", () => {
    const docs = buildDiffDocuments(fileAt(parseUnifiedDiff(MULTI_FILE), 0))

    expect(docs.original.split("\n")).toEqual([
      "//! crate docs",
      "pub fn old() {}",
      "",
      "    #[test]",
      "    fn a() {}",
    ])
    expect(docs.doc.split("\n")).toEqual([
      "//! crate docs",
      "pub fn renamed() {}",
      "pub fn added() {}",
      "",
      "    #[test]",
      "    fn b() {}",
    ])
  })

  it("carries the new-file line numbers, which the document cannot imply", () => {
    const docs = buildDiffDocuments(fileAt(parseUnifiedDiff(MULTI_FILE), 0))

    expect(docs.lineNumbers).toEqual([1, 2, 3, 4, 21, 22])
    expect(docs.hunkStarts.map((hunk) => hunk.line)).toEqual([0, 4])
    expect(docs.lineCount).toBe(6)
  })

  it("leaves the new document empty for a deleted file", () => {
    const docs = buildDiffDocuments(fileAt(parseUnifiedDiff(MULTI_FILE), 2))

    expect(docs.doc).toBe("")
    expect(docs.original).toBe("gone\nalso gone")
  })
})
