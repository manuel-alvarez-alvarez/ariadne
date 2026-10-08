---
id: issues-and-goals
status: current
updated: 2026-10-08
areas: [api, daemon, store, cli, ui, prompts]
commits: []
tests:
  - crates/ariadne-daemon/tests/it/issues.rs
  - crates/ariadne-daemon/tests/it/prompts.rs
  - crates/ariadne-store/tests/store.rs
  - crates/ariadne-cli/src/commands/goal.rs
  - crates/ariadne-cli/src/commands/issue.rs
  - crates/ariadne-cli/src/cli/tests.rs
  - ui/src/features/issues/issues-page.test.tsx
  - crates/ariadne-daemon/tests/it/pull_requests.rs
  - ui/src/events/dispatch.test.ts
---

# Issues and goals

## Scope

In: reading issues from enabled forge repositories and creating a goal from one.
Out: forge detection and enablement (025), goal planning (003), and the landing procedure.
The CLI and desktop conventions are in 014 and 015.

## Behavior

1. `GET /v1/repositories/{id}/issues` reads open issues from the forge each time. `assigned=me` is the default and filters by the stored forge login. `assigned=all` reads all open issues. No issue is stored locally.
2. `GET /v1/repositories/{id}/issues/{number}` reads one issue. Both routes refuse a repository whose forge integration is off with 409.
3. Each issue has its number, title, body, URL, labels, assignees, and update time. GitHub uses `gh issue list` and `gh issue view`; GitLab uses `glab issue list` and `glab issue view`.
4. A goal can store a nullable `issue_url`. Its DTO carries the URL. An orchestrator briefing for such a goal names the issue and requires every author request body to say `Closes <url>`.
5. `ariadne issue ls --repo <id|path> [--all]` reads the issue list. `ariadne goal create --from-issue <url> --model ...` reads the issue detail and uses its title and body when `--title` and `-d` are absent. `ariadne goal inspect` prints the issue URL.
6. The Issues tab of the Forge screen, at `#/forge/issues`, lists issues from enabled repositories. It filters by repository and has an Assigned to me switch on by default, both kept in the URL. Each row shows the linked issue number and title with the repository under it, the labels as pills, the assignees, and the update age.
7. Each repository fetch (026 rule 6) reads the open issues too, and publishes `issues_changed` with the repository id where they moved since that fetch worker's last read (012). A webhook delivery of an issue event wakes that fetch (027). The desktop reads that repository's issue lists again on the event, both assignment filters; the issues of other repositories stay.
8. A text box narrows the Issues tab to the words of the number, title, body, labels or assignees, kept in the URL. Refresh reads the issues from the forge again.
9. Create goal on an issue opens the existing dialog with its title, body, issue URL, and repository. The description ends with a blank line and `Issue: <url>`. The goal panel links to the issue.

## Acceptance criteria

- The live route reads two stub issues, passes the login for `me`, and refuses an integration that is off (`issues.rs::open_issues_are_live_and_assigned_uses_the_forge_login`).
- GitLab issue reads pass the group path and login to `glab` (`issues.rs::gitlab_issues_use_glab_json_and_the_group_path`).
- A goal stores and returns `issue_url`, and its orchestrator briefing names the issue and `Closes` line (`prompts.rs::an_issue_goal_keeps_its_url_and_briefs_the_orchestrator_to_close_it`).
- The nullable migration retains existing goals (`store.rs::the_issue_url_migration_keeps_existing_goals`).
- The CLI creates a goal using the issue title and body (`goal.rs::create_from_issue_reads_its_title_and_body_through_the_route`).
- The CLI issue list selects the repository and assignment filter (`issue.rs::issue_ls_uses_the_repository_and_assignment_filter`).
- The CLI accepts issue creation and issue listing flags (`cli/tests.rs::an_issue_can_supply_the_goal_title_and_repository`).
- The Issues screen lists an issue and opens a filled goal dialog (`issues-page.test.tsx::lists forge issues and fills the goal dialog from a chosen issue`).
- It asks for the issues assigned to me first, and for every open one once the switch is off, and Refresh reads them again (`issues-page.test.tsx::asks for issues assigned to me first, and for every open one once the switch is off`).
- It narrows the issues by text (`issues-page.test.tsx::narrows the issues to the words of their title or description`).
- A fetch publishes `issues_changed` only when the open issues moved (`pull_requests.rs::a_fetch_publishes_issues_changed_only_when_the_open_issues_moved`).
- The event refetches both assignment filters of that repository and no other (`dispatch.test.ts::issue events > refetches both assignment filters of the repository whose issues moved, and no other`).

## Sources

`crates/ariadne-daemon/src/forge/`, `crates/ariadne-daemon/src/http/issues.rs`,
`crates/ariadne-api/src/issues.rs`, `crates/ariadne-store/migrations/0005_goal_issue_url.sql`,
`crates/ariadne-cli/src/commands/issue.rs`, `crates/ariadne-daemon/src/forge/poll.rs`,
`ui/src/features/issues/issues-page.tsx`, `ui/src/events/dispatch.ts`.
