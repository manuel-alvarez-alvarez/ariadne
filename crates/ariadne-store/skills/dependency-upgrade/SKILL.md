---
name: dependency-upgrade
description: Raise one dependency with its changes and proof. Use when dependency versions rise, changelogs change, or lockfiles need updates.
---

# Dependency upgrade

The upgrade changelog is the work. The upgrade version bump is one line.

## Steps

1. Record the current version and the target version.
   Done when the upgrade range is written down.
2. Read every changelog entry in the upgrade range.
   Done when you list every breaking or behavior change.
3. Find every affected call site in this repository.
   Done when each upgrade change names its call sites.
4. Raise the dependency and fix its affected call sites together.
   Done when the manifest and lockfile hold the target version.
5. Run the full suite, linters and build.
   Done when every upgrade check passes.
6. Exercise the system path that uses the dependency most.
   Done when that path shows the expected upgrade behavior.

## Rules

- Upgrade one dependency per task.
- Read the upgrade changelog. Never skip it because the tests pass.
- Keep the lockfile in the upgrade commit.
- Give an upgrade that needs large code changes its own task.

## Do not tell yourself

- "The tests pass, so the upgrade is safe." -> Read every changelog entry.
- "A patch changes nothing." -> Exercise the main dependency path.
- "The lockfile is generated." -> Keep it with the upgrade manifest.

## Done

The dependency version is raised. Every upgrade change is answered. The
upgrade checks pass. The report names behavior changes.
