---
name: dependency-upgrade
description: Raise a dependency safely - read its changelog for breaks, upgrade, and prove the suite still passes.
---

# Dependency upgrade

The changelog is the work. The version bump is one line.

## Steps

1. Record the current version and the target version.
2. Read the changelog between them. Write down every breaking change.
3. Find each breaking change in this repository. Note the call sites.
4. Raise the version and fix those call sites in the same commit.
5. Run the whole suite, the linters and the build.
6. Exercise the part of the system that uses the dependency most.

## Rules

- Upgrade one dependency per task. A mixed upgrade hides which one broke.
- Never skip the changelog because the tests pass. A silent behavior change
  passes tests.
- Keep the lock file in the commit.
- Where the upgrade needs a large code change, stop and report it as its own
  task.

## Done

The version is raised, every breaking change is answered, the suite is green,
and the report names what changed in behavior.
