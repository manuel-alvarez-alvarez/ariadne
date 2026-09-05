---
name: spec-writing
description: Write a specification for a problem - its scope, its behavior, and the acceptance criteria that prove it.
---

# Spec writing

Write what the system does, not how the code does it. A reader must be able to
test every claim you make.

## Steps

1. Read the goal and the code it touches. List what you do not know.
2. Ask the user about each unknown. Ask one question at a time.
3. Write the scope. Name what is in it. Name what is out of it, and say where
   that belongs instead.
4. Write the behavior as numbered sentences. State one rule per sentence.
5. Write one acceptance criterion per rule. Name the test that proves it.
6. Follow the format of the specs already in the repository. Where there are
   none, agree a path and a format with the user first.

## Rules

- State each rule once. A rule in two places goes stale in one of them.
- Write what holds now, not what is planned. A spec describes; it does not
  promise.
- Give every number a reason. An unexplained limit is one nobody can change
  safely.
- Leave out anything you cannot test.

## Done

The spec names its scope, its rules and its criteria. Every criterion names a
test. The user agreed to it in writing.
