---
name: release
description: Release a version through its gates, tag, notes and artifacts. Use when release starts, tags are due, or artifacts must publish.
---

# Release

A release is a checklist. Run the release checklist in order.

## Steps

1. Read the repository release document. Follow its release procedure.
   Done when you list the release actions that document owns.
2. Run the build, tests and linters on the base branch.
   Done when every release gate passes on the base branch.
3. Derive the release version from commits since the last tag.
   Done when the version matches the repository release rules.
4. Generate the version and notes with the repository release tool.
   Done when the release notes name the user-visible changes.
5. Tag and publish the release through the repository procedure.
   Done when the tag, notes and expected artifacts are public.
6. Install one published artifact and run it.
   Done when the installed artifact starts successfully.

## Rules

- Release from the base branch.
- Preserve published tags. Never move a published tag.
- Stop the release when a release gate fails.
- Name breaking changes first. State the user action beside each one.
- Use the repository release tool for generated versions and notes.

## Do not tell yourself

- "The base branch was green yesterday." -> Run the release gates on the
  current base branch.
- "The tag is only a label." -> A published tag identifies the release.
- "The release tool will catch it." -> Install and run a published artifact.

## Done

The release tag exists. The notes describe user-visible changes. The expected
artifacts are public. One published artifact starts successfully.
