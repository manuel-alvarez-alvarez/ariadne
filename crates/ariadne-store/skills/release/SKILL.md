---
name: release
description: Take a version out - the checks that gate it, the version, the changelog, the tag and the notes.
---

# Release

A release is a checklist. Run all of it, in order.

## Steps

1. Read the repository's own release document. It wins over this skill.
2. Confirm the base branch is green: tests, linters and the build.
3. Work out the version from the commits since the last tag.
4. Update the changelog from those commits. Write what changed for a user.
5. Tag the release and push the tag.
6. Publish the notes and the artifacts the repository expects.
7. Verify what was published. Install one artifact and run it.

## Rules

- Release from the base branch, never from a task branch.
- Never move a tag that is already published.
- Where a check is red, stop. Do not release around a failure.
- Name breaking changes first in the notes, with what a user does about them.
- Where a tool generates the version and the notes, let it. Do not also write
  them by hand.

## Done

The tag exists, the notes describe the change for a user, the artifacts are
published, and one of them was installed and run.
