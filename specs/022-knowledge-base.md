---
id: knowledge-base
status: current
updated: 2026-09-18
areas: [daemon, cli, ui]
commits: []
tests:
  - ui/src/features/knowledge/knowledge-page.test.tsx
  - ui/src/events/dispatch.test.ts
---

# Knowledge base

Ariadne indexes every registered repository, so an agent, the CLI and the
desktop app can ask what is in it without rereading the whole checkout.

## Scope

In: one repository's index status, a reindex, a search over what was
indexed, the interactions found between files, the REST routes and domain
events behind them (012), the CLI commands (014), and the desktop screen
that offers the same four (015).

Out: what the indexer itself parses and how — that is the daemon's own
implementation, not a contract this spec fixes.

## Behavior

1. A repository's knowledge base has one state: `idle`, `indexing`, `failed`
   or `disabled`. `GET /v1/repositories/{id}/knowledge` answers it, along
   with every indexed ref (its git ref, commit and when it was indexed),
   file and symbol counts, a per-language file count, and the last error
   where the state is `failed`.
2. `POST /v1/repositories/{id}/knowledge/reindex` answers `202` and starts a
   rebuild in the background; the caller learns it finished from the
   `knowledge_indexed` or `knowledge_failed` event, not from the response.
3. `GET /v1/knowledge/search` answers matches across one or every indexed
   repository, filtered by `q`, `repository`, `git_ref`, `kind` and `path`,
   each optional; a match carries its repository, path, line, kind, name and
   signature.
4. `GET /v1/knowledge/interactions` answers the edges found between files,
   grouped by kind (`depends_on`, `references`, `calls_route`, `sets_env`),
   filtered by `repository` and `git_ref`; each edge names both ends
   (repository, path, line, symbol) and whether it was found exactly or by a
   heuristic.
5. `knowledge_indexed` (`repository_id`, `git_ref`, `commit`, `files`,
   `symbols`) and `knowledge_failed` (`repository_id`, `error`) are domain
   events (012), so every open client learns of a finished or failed
   reindex without polling.
6. The desktop app's knowledge page (015) shows the status card, a Reindex
   button that posts the reindex and shows `indexing` at once, a search box
   over `q`/`kind`/`path`, and the interactions grouped by kind — reached
   from a row on the repositories screen and from the command palette, the
   way the memory page (019) is reached from its row.

## Acceptance criteria

- The desktop knowledge page renders the status card in every state, posts a
  reindex and shows `indexing` at once, refetches once `knowledge_indexed` or
  `knowledge_failed` arrives, searches with `q`, `kind` and `path`, and groups
  interactions by kind with both ends and their confidence
  (`ui/src/features/knowledge/knowledge-page.test.tsx`).
- `knowledge_indexed` and `knowledge_failed` invalidate a repository's
  knowledge status and every interactions list under it, and leave another
  repository's caches alone
  (`ui/src/events/dispatch.test.ts::knowledge events (022)`).
- The command palette opens a repository's knowledge page
  (`ui/src/features/command-palette/command-palette.test.tsx::opens a
  repository's knowledge page from the palette`).

## Sources

`crates/ariadne-daemon` (routes and indexing), `crates/ariadne-cli` (the
`knowledge` command, 014), `ui/src/features/knowledge/` (015).
