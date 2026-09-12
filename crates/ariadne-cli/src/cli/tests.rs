//! The command tree as a conformance table: every leaf classified, and the
//! spellings scripts depend on pinned to the field they land in.

use super::*;

use clap::FromArgMatches;

use ariadne_core::{GoalStatus, Landing, PermissionMode, Seat, SessionStatus, TaskStatus};

use crate::commands::models::ModelsCommand;
use crate::commands::skill::SkillCommand;
use crate::output::ColorChoice;

/// clap's own consistency check over the whole tree, shadowed `--format`
/// arguments included.
#[test]
fn the_command_tree_is_well_formed() {
    command().debug_assert();
}

/// The command groups: every one of them is a screen someone lands on from
/// `ariadne --help`, so every one of them has to read the same way.
const GROUPS: &[&str] = &[
    "agent", "daemon", "goal", "memory", "repo", "session", "skill", "task",
];

/// The root and every group say what they are for, list the two global flags
/// under a heading of their own, and end in examples. A screen missing one of
/// those is a screen someone has to guess the rest of.
#[test]
fn the_root_and_every_group_are_one_help_screen_shape() {
    let mut screens = vec![vec![]];
    screens.extend(GROUPS.iter().map(|group| vec![*group]));
    for path in screens {
        let name = path.join(" ");
        let help = long_help(&path);
        assert!(help.contains("Global options:"), "{name}: {help}");
        assert!(help.contains("\nExamples:\n"), "{name}: no examples");
        assert!(
            help.contains("--endpoint <ENDPOINT>") && help.contains("--format <FORMAT>"),
            "{name}: {help}"
        );
        assert!(
            subcommand(&path).get_long_about().is_some(),
            "{name}: nothing says what it is for"
        );
    }
}

/// `--endpoint` reads the environment where the endpoint is resolved rather
/// than through clap, so no help screen carries the value this shell happens
/// to have and no usage line reads as though the flag were required — which
/// is exactly what an `env =` on it used to do wherever `ARIADNE_SOCKET` was
/// set.
#[test]
fn no_help_screen_leaks_the_endpoint_of_the_shell_it_runs_in() {
    for path in [vec![], vec!["goal"], vec!["repo"], vec!["task", "create"]] {
        let name = path.join(" ");
        let help = long_help(&path);
        assert!(!help.contains("[env:"), "{name}: {help}");
        let usage = help
            .lines()
            .find(|line| line.starts_with("Usage:"))
            .unwrap_or_else(|| panic!("{name}: no usage line"));
        assert!(!usage.contains("--endpoint"), "{name}: {usage}");
    }
}

/// A description nobody gave is empty, which is what the daemon is sent; the
/// help has no business printing `[default: ""]` at a reader.
#[test]
fn an_empty_description_is_not_advertised_as_a_default() {
    for path in [["goal", "create"], ["task", "create"]] {
        let help = long_help(&path);
        assert!(help.contains("-d, --description"), "{help}");
        assert!(!help.contains("[default: \"\"]"), "{help}");
    }
}

/// Every command a user can actually run, and whether `--format` shapes
/// what it prints. Hidden internal commands are in here too: `--format`
/// is global, so it reaches them whether or not anyone meant it to.
const LEAVES: &[(&str, bool)] = &[
    ("agent ls", true),
    ("agent update", true),
    ("attach", false),
    ("attention", true),
    ("completions", false),
    ("completions install", false),
    ("daemon logs", false),
    ("daemon restart", true),
    ("daemon start", true),
    ("daemon status", true),
    ("daemon stop", true),
    ("doctor", true),
    ("events", true),
    ("goal attach", false),
    ("goal cancel", true),
    ("goal complete", true),
    ("goal create", true),
    ("goal inspect", true),
    ("goal ls", true),
    ("goal rm", true),
    ("mcp serve", false),
    ("memory delete", true),
    ("memory ls", true),
    ("memory search", true),
    ("models disable", true),
    ("models enable", true),
    ("models ls", true),
    ("models show", true),
    ("repo add", true),
    ("repo inspect", true),
    ("repo ls", true),
    ("repo rm", true),
    ("repo update", true),
    ("session adopt", true),
    ("session discover", true),
    ("session inspect", true),
    ("session kill", true),
    ("session logs", true),
    ("session ls", true),
    ("session resume", true),
    ("session send", true),
    ("skill create", true),
    ("skill get", true),
    ("skill inspect", true),
    ("skill ls", true),
    ("skill reset", true),
    ("skill rm", true),
    ("skill set", true),
    ("task attach", false),
    ("task cancel", true),
    ("task create", true),
    ("task diff", true),
    ("task history", true),
    ("task inspect", true),
    ("task logs", true),
    ("task ls", true),
    ("task retry", true),
    ("task messages", true),
    ("task update", true),
    ("version", true),
];

/// The list above is the whole tree, so a command added later is not
/// classified by accident: it fails here until someone says whether
/// `--format` means anything to it.
#[test]
fn every_command_in_the_tree_is_classified() {
    let mut expected: Vec<&str> = LEAVES.iter().map(|(path, _)| *path).collect();
    expected.sort_unstable();
    assert_eq!(leaf_paths(), expected);
}

#[test]
fn format_is_advertised_exactly_where_it_is_honored() {
    let cmd = built();
    for (path, honored) in LEAVES {
        let path: Vec<&str> = path.split(' ').collect();
        assert_eq!(
            advertises(&cmd, &path, "format"),
            *honored,
            "--format is {} on {path:?}",
            if *honored { "missing" } else { "advertised" }
        );
    }
}

/// Table flags belong to listings, quiet belongs to listings and mutations,
/// and the pager flag belongs to long output.
///
/// Every path they name is checked against the real tree at the same time: a
/// renamed subcommand would otherwise quietly stop advertising its own flags.
#[test]
fn the_listing_flags_are_advertised_exactly_where_they_are_honored() {
    let cmd = built();
    let leaves = leaf_paths();
    for named in LISTINGS.iter().chain(PAGED) {
        assert!(
            leaves.iter().any(|leaf| leaf == named),
            "no such command: {named}"
        );
    }
    for leaf in &leaves {
        let path: Vec<&str> = leaf.split(' ').collect();
        let listing = LISTINGS.contains(&leaf.as_str());
        for id in ["no_trunc", "layout", "columns"] {
            assert_eq!(advertises(&cmd, &path, id), listing, "{id} on {leaf:?}");
        }
        assert_eq!(
            advertises(&cmd, &path, "quiet"),
            QUIET_OUTPUT.contains(&leaf.as_str()),
            "quiet on {leaf:?}"
        );
        assert_eq!(
            advertises(&cmd, &path, "no_pager"),
            PAGED.contains(&leaf.as_str()),
            "--no-pager on {leaf:?}"
        );
        // Colour goes wherever output does, which is wherever `--format`
        // does: the two answer the same question about the same commands.
        assert_eq!(
            advertises(&cmd, &path, "color"),
            advertises(&cmd, &path, "format"),
            "--color on {leaf:?}"
        );
    }
}

/// `--watch` belongs to the commands whose table is a live picture of state —
/// redrawn on every event through `follow::watch` — and nowhere else, so
/// `ariadne task cancel --help` never advertises a flag it ignores.
///
/// Every path [`WATCHED`] names is checked against the real tree too: a
/// renamed subcommand would otherwise quietly stop advertising its own flag.
#[test]
fn the_watch_flag_is_advertised_exactly_where_it_is_honored() {
    let cmd = built();
    let leaves = leaf_paths();
    for named in WATCHED {
        assert!(
            leaves.iter().any(|leaf| leaf == named),
            "no such command: {named}"
        );
    }
    for leaf in &leaves {
        let path: Vec<&str> = leaf.split(' ').collect();
        assert_eq!(
            advertises(&cmd, &path, "watch"),
            WATCHED.contains(&leaf.as_str()),
            "--watch on {leaf:?}"
        );
    }
}

/// The display flags are global, so they may be typed before the subcommand
/// or after it, and every one of them lands in the field the renderer reads.
#[test]
fn the_display_flags_parse_on_either_side_of_the_subcommand() {
    let before = parse(&[
        "ariadne",
        "--color",
        "never",
        "--no-trunc",
        "-q",
        "-o",
        "wide",
        "--columns",
        "id,title",
        "task",
        "ls",
    ]);
    let after = parse(&[
        "ariadne",
        "task",
        "ls",
        "--color",
        "never",
        "--no-trunc",
        "-q",
        "-o",
        "wide",
        "--columns",
        "id,title",
    ]);
    for cli in [before, after] {
        assert_eq!(cli.color, ColorChoice::Never);
        assert!(cli.no_trunc);
        assert!(cli.quiet);
        assert_eq!(cli.layout, Layout::Wide);
        assert_eq!(cli.columns, ["id", "title"]);
    }

    let plain = parse(&["ariadne", "task", "ls"]);
    assert_eq!(plain.color, ColorChoice::Auto);
    assert_eq!(plain.layout, Layout::Normal);
    assert!(!plain.quiet && !plain.no_trunc && !plain.no_pager);
    assert!(plain.columns.is_empty());
    assert!(
        try_parse(&["ariadne", "task", "ls", "--color", "purple"]).is_err(),
        "a colour choice is one of three words"
    );
}

#[test]
fn quiet_parses_after_a_mutation() {
    assert!(
        parse(&[
            "ariadne",
            "goal",
            "create",
            "-q",
            "--title",
            "x",
            "--repo",
            "r",
            "--model",
            "codex-acp:m",
        ])
        .quiet
    );
    assert!(parse(&["ariadne", "session", "send", "01SESSION", "yes", "-q"]).quiet);
}

#[test]
fn discover_takes_filters_pages_refresh_and_all() {
    let Command::Session {
        command:
            SessionCommand::Discover {
                agent,
                dir,
                since,
                until,
                search,
                limit,
                cursor,
                refresh,
                all,
            },
    } = parse(&[
        "ariadne",
        "session",
        "discover",
        "--agent",
        "codex-acp",
        "--dir",
        "/work/api",
        "--since",
        "2026-09-01",
        "--until",
        "2026-09-12T12:30:00+02:00",
        "--search",
        "rate limit",
        "--limit",
        "25",
        "--refresh",
        "--all",
    ])
    .command
    else {
        panic!("session discover");
    };

    assert_eq!(agent.as_deref(), Some("codex-acp"));
    assert_eq!(dir.as_deref(), Some("/work/api"));
    assert_eq!(since.as_deref(), Some("2026-09-01T00:00:00Z"));
    assert_eq!(until.as_deref(), Some("2026-09-12T12:30:00+02:00"));
    assert_eq!(search.as_deref(), Some("rate limit"));
    assert_eq!(limit, Some(25));
    assert_eq!(cursor, None);
    assert!(refresh);
    assert!(all);
}

/// `session adopt` takes the session, the agent it belongs to, the goal that
/// receives the work, and the task flags `task create` takes — one author,
/// the reviewers in review order, and the two enums a task ends on.
#[test]
fn adopt_takes_the_session_the_new_goal_and_the_task_flags() {
    let Command::Session {
        command:
            SessionCommand::Adopt {
                session_id,
                agent,
                goal,
                new_goal,
                goal_description,
                repos,
                title,
                description,
                author,
                reviewers,
                no_reviewer,
                landing,
                permission_mode,
            },
    } = parse(&[
        "ariadne",
        "session",
        "adopt",
        "codex-outside-1",
        "--agent",
        "codex-acp",
        "--new-goal",
        "Finish the rate limiter",
        "--goal-description",
        "What the outside session started",
        "--repo",
        "01REPO",
        "--repo",
        "/work/ui",
        "--title",
        "Wire the limiter in",
        "-d",
        "The brief",
        "--author",
        "coding,testing=codex-acp:gpt-5.6-sol@xhigh",
        "--reviewer",
        "code-review=claude-agent-acp:claude-opus-5@high",
        "--reviewer",
        "security-review=codex-acp:gpt-5.6-luna",
        "--landing",
        "pull-request",
        "--permission-mode",
        "learn",
    ])
    .command
    else {
        panic!("session adopt");
    };

    assert_eq!(session_id, "codex-outside-1");
    assert_eq!(agent, "codex-acp");
    assert_eq!(goal, None);
    assert_eq!(new_goal.as_deref(), Some("Finish the rate limiter"));
    assert_eq!(
        goal_description.as_deref(),
        Some("What the outside session started")
    );
    assert_eq!(repos, ["01REPO", "/work/ui"]);
    assert_eq!(title.as_deref(), Some("Wire the limiter in"));
    assert_eq!(description, "The brief");
    assert_eq!(author.skills, ["coding", "testing"]);
    assert_eq!(author.model, "codex-acp:gpt-5.6-sol");
    assert_eq!(author.effort.as_deref(), Some("xhigh"));
    assert_eq!(
        reviewers
            .iter()
            .map(|r| (r.skills.join(","), r.model.as_str()))
            .collect::<Vec<_>>(),
        [
            ("code-review".to_string(), "claude-agent-acp:claude-opus-5"),
            ("security-review".to_string(), "codex-acp:gpt-5.6-luna"),
        ]
    );
    assert!(!no_reviewer);
    assert_eq!(landing, Some(Landing::PullRequest));
    assert_eq!(permission_mode, Some(PermissionMode::Learn));
}

/// An adopted task lands in one place: a goal that is already active, or a new
/// one. A line that names both places, or neither, says nothing about where.
#[test]
fn adopt_takes_a_goal_or_a_new_goal_and_exactly_one() {
    let line = |args: &[&str]| {
        let mut argv = vec![
            "ariadne",
            "session",
            "adopt",
            "codex-outside-1",
            "--agent",
            "codex-acp",
            "--author",
            "coding=codex-acp:gpt-5.6-sol",
        ];
        argv.extend_from_slice(args);
        try_parse(&argv)
    };

    assert!(line(&["--goal", "01GOAL"]).is_ok());
    assert!(line(&["--new-goal", "Finish the rate limiter"]).is_ok());
    assert!(
        line(&["--goal", "01GOAL", "--new-goal", "Finish it"]).is_err(),
        "one adoption cannot open a goal and join one"
    );
    assert!(line(&[]).is_err(), "nothing says where the task belongs");
    assert!(
        line(&["--goal", "01GOAL", "--goal-description", "Why"]).is_err(),
        "an existing goal has its description already"
    );
    assert!(
        try_parse(&[
            "ariadne",
            "session",
            "adopt",
            "codex-outside-1",
            "--agent",
            "codex-acp",
            "--goal",
            "01GOAL",
        ])
        .is_err(),
        "nobody writes the task without an author"
    );
}

#[test]
fn discover_all_and_cursor_are_exclusive() {
    assert!(
        try_parse(&[
            "ariadne",
            "session",
            "discover",
            "--all",
            "--cursor",
            "next-page",
        ])
        .is_err()
    );
}

#[test]
fn memory_delete_takes_the_entry_and_its_repository() {
    let Command::Memory {
        command: MemoryCommand::Delete { id, repo, yes },
    } = parse(&[
        "ariadne", "memory", "delete", "01MEMORY", "--repo", "01REPO", "--yes",
    ])
    .command
    else {
        panic!("memory delete");
    };
    assert_eq!(id, "01MEMORY");
    assert_eq!(repo, "01REPO");
    assert!(yes);
}

/// Every `ls` that hides finished work behind `--all` takes the same short
/// flag for it.
#[test]
fn a_listing_hides_what_is_finished_behind_the_same_flag() {
    let all = |argv: &[&str]| match parse(argv).command {
        Command::Goal {
            command: GoalCommand::Ls { all, .. },
        } => all,
        Command::Task {
            command: TaskCommand::Ls { all, .. },
        } => all,
        Command::Session {
            command: SessionCommand::Ls { all, .. },
        } => all,
        _ => panic!("ls"),
    };
    for argv in [
        vec!["ariadne", "goal", "ls"],
        vec!["ariadne", "task", "ls"],
        vec!["ariadne", "session", "ls"],
    ] {
        assert!(!all(&argv), "{argv:?} lists what is going on");
        let mut with = argv.clone();
        with.push("-a");
        assert!(all(&with), "{with:?} lists everything");
    }
}

/// A path in [`NO_FORMAT`] that names no command hides nothing —
/// `mut_subcommand` shrugs at a name it does not know.
#[test]
fn every_no_format_path_names_a_real_command() {
    let mut cmd = command();
    cmd.build();
    for path in NO_FORMAT {
        let mut sub = &cmd;
        for name in *path {
            sub = sub
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("NO_FORMAT names no command: {path:?}"));
        }
    }
}

/// Every filter takes the daemon's own spelling and nothing else, so a value
/// that is not one is clap's refusal here rather than a request the daemon has
/// to turn down — and `goal ls --status` takes as many as the caller likes.
#[test]
fn a_filter_takes_only_the_values_the_daemon_knows() {
    let statuses = |args: &[&str]| {
        let mut argv = vec!["ariadne", "goal", "ls"];
        argv.extend_from_slice(args);
        let Command::Goal {
            command: GoalCommand::Ls { statuses, .. },
        } = parse(&argv).command
        else {
            panic!("goal ls");
        };
        statuses
    };
    assert_eq!(statuses(&[]), []);
    assert_eq!(
        statuses(&["--status", "active", "--status", "completed"]),
        [GoalStatus::Active, GoalStatus::Completed]
    );
    assert_eq!(
        statuses(&["--status", "planning"]),
        [GoalStatus::Planning],
        "a status is spelled on the command line the way the daemon spells it"
    );
    let Err(err) = try_parse(&["ariadne", "goal", "ls", "--status", "done"]) else {
        panic!("\"done\" is not a goal status");
    };
    let msg = err.to_string();
    assert!(msg.contains("invalid value 'done'"), "{msg}");
    assert!(msg.contains("completed"), "the refusal lists the real ones");

    let Command::Session {
        command: SessionCommand::Ls { seat, .. },
    } = parse(&["ariadne", "session", "ls", "--seat", "reviewer"]).command
    else {
        panic!("session ls");
    };
    assert_eq!(seat, Some(Seat::Reviewer));
    assert!(
        try_parse(&["ariadne", "session", "ls", "--seat", "critic"]).is_err(),
        "an unknown seat is a usage error"
    );

    let Command::Task {
        command: TaskCommand::Ls { statuses, .. },
    } = parse(&["ariadne", "task", "ls", "--status", "approved"]).command
    else {
        panic!("task ls");
    };
    assert_eq!(statuses, [TaskStatus::Approved]);
    assert!(
        try_parse(&["ariadne", "task", "ls", "--status", "integrating"]).is_err(),
        "and the status a task was landed from by a fourth seat is gone"
    );
}

/// A status is typed either way round: the kebab-case spelling the help
/// prints and the completions offer, and the snake_case one the daemon, the
/// API and `--format json` answer with — so a status read off a listing is a
/// status that can be typed straight back in.
#[test]
fn a_status_is_spelled_in_kebab_or_in_snake() {
    assert_eq!(task_statuses(&["in-progress"]), [TaskStatus::InProgress]);
    assert_eq!(task_statuses(&["in_progress"]), [TaskStatus::InProgress]);
    let Command::Task {
        command:
            TaskCommand::Create {
                landing,
                permission_mode,
                ..
            },
    } = parse(&[
        "ariadne",
        "task",
        "create",
        "01GOAL",
        "--title",
        "t",
        "--author",
        "coding=claude-agent-acp:claude-sonnet-5",
        "--landing",
        "pull-request",
        "--permission-mode",
        "learn",
    ])
    .command
    else {
        panic!("task create");
    };
    assert_eq!(
        landing,
        Some(Landing::PullRequest),
        "and so is every other enum a flag takes"
    );
    assert_eq!(permission_mode, Some(PermissionMode::Learn));
}

/// Several statuses ride on one `ls`, comma-separated or on a flag each, and
/// every `ls` takes them the same way.
#[test]
fn several_statuses_ride_on_one_flag() {
    let both = [TaskStatus::InProgress, TaskStatus::UnderReview];
    assert_eq!(task_statuses(&["in-progress,under_review"]), both);
    assert_eq!(task_statuses(&["in-progress", "under-review"]), both);

    let Command::Session {
        command: SessionCommand::Ls { statuses, .. },
    } = parse(&["ariadne", "session", "ls", "--status", "idle,exited"]).command
    else {
        panic!("session ls");
    };
    assert_eq!(statuses, [SessionStatus::Idle, SessionStatus::Exited]);
}

/// A value that is neither spelling is refused where it was typed, quoted as
/// it was typed, with the spellings the help prints listed back.
#[test]
fn a_status_that_is_no_spelling_of_one_lists_the_real_ones() {
    let err = try_parse(&["ariadne", "task", "ls", "--status", "in progress"])
        .map(|_| ())
        .expect_err("no such status")
        .to_string();
    assert!(err.contains("invalid value 'in progress'"), "{err}");
    assert!(err.contains("in-progress"), "{err}");
}

/// The statuses one `task ls` line asked for.
fn task_statuses(values: &[&str]) -> Vec<TaskStatus> {
    let mut argv = vec!["ariadne", "task", "ls"];
    for value in values {
        argv.extend_from_slice(&["--status", value]);
    }
    let Command::Task {
        command: TaskCommand::Ls { statuses, .. },
    } = parse(&argv).command
    else {
        panic!("task ls");
    };
    statuses
}

/// What each agent runs on is chosen on the way in, as one string: `--model`
/// for the orchestrator and the author, `--reviewer SKILLS=MODEL` per
/// reviewer slot, and every spelling lands in the field the request is built
/// from. A model is required, so every spelling carries one.
#[test]
fn a_model_can_be_chosen_for_every_agent_on_the_line() {
    let orchestrator = |args: &[&str]| {
        let mut argv = vec![
            "ariadne", "goal", "create", "--title", "Ship it", "--repo", "01REPO",
        ];
        argv.extend_from_slice(args);
        let Command::Goal {
            command: GoalCommand::Create { model, .. },
        } = parse(&argv).command
        else {
            panic!("goal create")
        };
        model
    };
    assert_eq!(
        orchestrator(&["--model", "codex-acp:gpt-5.3-codex"]),
        "codex-acp:gpt-5.3-codex",
        "a model travels as it was typed"
    );

    let Command::Task {
        command: TaskCommand::Create {
            authors, reviewers, ..
        },
    } = parse(&[
        "ariadne",
        "task",
        "create",
        "01GOAL",
        "--title",
        "Do it",
        "--author",
        "coding,testing=claude-agent-acp:claude-opus-5",
        "--reviewer",
        "code-review=codex-acp:o3",
        "--reviewer",
        "security-review=opencode-acp:ollama/llama3:8b",
    ])
    .command
    else {
        panic!("task create")
    };
    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].skills, ["coding", "testing"]);
    assert_eq!(authors[0].model, "claude-agent-acp:claude-opus-5");
    assert_eq!(
        reviewers
            .iter()
            .map(|r| (r.skills.join(","), r.model.as_str()))
            .collect::<Vec<_>>(),
        [
            ("code-review".to_string(), "codex-acp:o3"),
            (
                "security-review".to_string(),
                "opencode-acp:ollama/llama3:8b"
            ),
        ],
        "in the order they were typed, which is review order"
    );

    let edited = |args: &[&str]| {
        let mut argv = vec!["ariadne", "task", "update", "01TASK"];
        argv.extend_from_slice(args);
        let Command::Task {
            command: TaskCommand::Update { model, .. },
        } = parse(&argv).command
        else {
            panic!("task update")
        };
        model
    };
    assert_eq!(
        edited(&["--model", "codex-acp:gpt-5.3-codex"]).as_deref(),
        Some("codex-acp:gpt-5.3-codex")
    );
    assert_eq!(edited(&["--title", "Do it better"]), None);
}

/// A model is required wherever an agent is chosen: `goal create` refuses a
/// line with no `--model`, `task create` a line with no `--author`, an agent
/// slot the `=MODEL` half, and `task update --model` the word `default` —
/// there is no longer anything to hand a pin back to.
#[test]
fn a_line_with_no_model_is_a_usage_error() {
    assert!(
        try_parse(&[
            "ariadne", "goal", "create", "--title", "Ship it", "--repo", "01REPO",
        ])
        .is_err(),
        "goal create parses with no --model"
    );

    assert!(
        try_parse(&["ariadne", "task", "create", "01GOAL", "--title", "Do it"]).is_err(),
        "task create parses with no --author"
    );

    let err = try_parse(&[
        "ariadne",
        "task",
        "create",
        "01GOAL",
        "--title",
        "Do it",
        "--author",
        "coding,testing",
    ])
    .map(|_| ())
    .expect_err("an author with no model")
    .to_string();
    assert!(err.contains("a model is required"), "{err}");
    assert!(err.contains("SKILLS=MODEL"), "{err}");

    // A bare agent parses nowhere: it names no model.
    let err = try_parse(&[
        "ariadne",
        "task",
        "update",
        "01TASK",
        "--model",
        "codex-acp",
    ])
    .map(|_| ())
    .expect_err("a bare agent")
    .to_string();
    assert!(err.contains("`codex-acp` names no agent"), "{err}");
    assert!(err.contains("a model is required"), "{err}");

    let err = try_parse(&["ariadne", "task", "update", "01TASK", "--model", "default"])
        .map(|_| ())
        .expect_err("default is no model")
        .to_string();
    assert!(err.contains("names no agent"), "{err}");

    // Whitespace after the colon is an empty model too: it would create a
    // pin and launch a model no agent has.
    let err = try_parse(&[
        "ariadne",
        "task",
        "update",
        "01TASK",
        "--model",
        "codex-acp: ",
    ])
    .map(|_| ())
    .expect_err("whitespace is no model")
    .to_string();
    assert!(err.contains("no model after the `:`"), "{err}");
    assert!(err.contains("a model is required"), "{err}");
}

/// The other half of a pin, on every line a model is chosen on: `--effort`
/// beside `--model`, `@EFFORT` on a reviewer slot, and the word an update
/// writes to run the model at whatever its CLI reasons it at.
#[test]
fn an_effort_can_be_chosen_beside_every_model() {
    let Command::Goal {
        command: GoalCommand::Create { model, effort, .. },
    } = parse(&[
        "ariadne",
        "goal",
        "create",
        "--title",
        "Ship it",
        "--repo",
        "01REPO",
        "--model",
        "codex-acp:gpt-5.6-sol",
        "--effort",
        "xhigh",
    ])
    .command
    else {
        panic!("goal create")
    };
    assert_eq!(model, "codex-acp:gpt-5.6-sol");
    assert_eq!(effort.as_deref(), Some("xhigh"));

    let Command::Task {
        command: TaskCommand::Create {
            authors, reviewers, ..
        },
    } = parse(&[
        "ariadne",
        "task",
        "create",
        "01GOAL",
        "--title",
        "Do it",
        "--author",
        "coding=claude-agent-acp:claude-opus-5@xhigh",
        "--reviewer",
        "code-review=codex-acp:gpt-5.6-sol@xhigh",
        "--reviewer",
        "security-review=claude-agent-acp:claude-sonnet-5@high",
        "--reviewer",
        "performance-review=codex-acp:gpt-5.6-luna",
    ])
    .command
    else {
        panic!("task create")
    };
    assert_eq!(authors[0].effort.as_deref(), Some("xhigh"));
    assert_eq!(
        reviewers
            .iter()
            .map(|r| (r.skills.join(","), r.model.as_str(), r.effort.as_deref()))
            .collect::<Vec<_>>(),
        [
            (
                "code-review".to_string(),
                "codex-acp:gpt-5.6-sol",
                Some("xhigh")
            ),
            (
                "security-review".to_string(),
                "claude-agent-acp:claude-sonnet-5",
                Some("high")
            ),
            (
                "performance-review".to_string(),
                "codex-acp:gpt-5.6-luna",
                None
            ),
        ],
        "an agent names its model, and the effort beside it where one was \
         chosen"
    );

    let edited = |args: &[&str]| {
        let mut argv = vec!["ariadne", "task", "update", "01TASK"];
        argv.extend_from_slice(args);
        let Command::Task {
            command: TaskCommand::Update { effort, .. },
        } = parse(&argv).command
        else {
            panic!("task update")
        };
        effort
    };
    assert_eq!(edited(&["--effort", "ultra"]).as_deref(), Some("ultra"));
    assert_eq!(edited(&["--effort", "default"]).as_deref(), Some("default"));
    assert_eq!(edited(&["--model", "codex-acp:gpt-5.3-codex"]), None);
}

/// An effort is the model's to accept, and the daemon holds the catalogue —
/// so the only thing the line itself refuses is a flag with no effort in it.
#[test]
fn an_effort_that_says_nothing_is_a_usage_error() {
    let err = try_parse(&["ariadne", "task", "update", "01TASK", "--effort", " "])
        .map(|_| ())
        .expect_err("no effort at all")
        .to_string();
    assert!(err.contains("no effort was named"), "{err}");
    assert!(err.contains("ariadne models ls"), "{err}");
    assert!(err.contains("default"), "{err}");

    // Which efforts a model takes is the daemon's answer, not this one's: an
    // effort no model of that agent runs at is still sent, and refused there.
    assert!(
        try_parse(&[
            "ariadne",
            "task",
            "update",
            "01TASK",
            "--model",
            "claude-agent-acp:claude-opus-5",
            "--effort",
            "ultra",
        ])
        .is_ok()
    );
}

/// A model does not say which agent runs it, so one that names no agent is
/// refused on the line it was typed on — with the spelling that would have
/// named one, never a request the daemon has to turn down.
#[test]
fn a_model_naming_no_agent_is_a_usage_error() {
    let lines: [&[&str]; 3] = [
        &[
            "ariadne",
            "goal",
            "create",
            "--title",
            "Ship it",
            "--repo",
            "01REPO",
            "--model",
            "gpt-5.3-codex",
        ],
        &[
            "ariadne",
            "task",
            "create",
            "01GOAL",
            "--title",
            "Do it",
            "--author",
            "coding=gpt-5.3-codex",
        ],
        &[
            "ariadne",
            "task",
            "create",
            "01GOAL",
            "--title",
            "Do it",
            "--reviewer",
            "code-review=gpt-5.3-codex",
        ],
    ];
    for argv in lines {
        let err = try_parse(argv)
            .map(|_| ())
            .expect_err("a model naming no agent")
            .to_string();
        assert!(err.contains("names no agent"), "{argv:?}: {err}");
        assert!(err.contains("`<agent>:gpt-5.3-codex`"), "{argv:?}: {err}");
    }

    // `default` is a word only an update takes: on a create it names no
    // agent either, since nothing is being handed back.
    assert!(
        try_parse(&[
            "ariadne", "goal", "create", "--title", "Ship it", "--repo", "01REPO", "--model",
            "default",
        ])
        .is_err()
    );
}

/// Which agents there are is the daemon's registry, which the line cannot
/// see: an agent id is sent as it was typed, and the daemon refuses one its
/// registry does not hold.
#[test]
fn an_agent_id_is_the_daemons_to_check() {
    assert!(
        try_parse(&["ariadne", "task", "update", "01TASK", "--model", "llama:x"]).is_ok(),
        "the line has no registry to check an agent against"
    );
}

/// A `--reviewer` that says half of what it means is a typo, and it is
/// refused where it was typed rather than sent to the daemon to be refused
/// there — with the form it accepts.
#[test]
fn a_reviewer_that_names_no_real_agent_is_a_usage_error() {
    let refused = |spec: &str| {
        try_parse(&[
            "ariadne",
            "task",
            "create",
            "01GOAL",
            "--title",
            "Do it",
            "--reviewer",
            spec,
        ])
        .map(|_| ())
        .expect_err("a reviewer that says half of what it means")
        .to_string()
    };
    let err = refused("code-review=llama");
    assert!(err.contains("names no agent"), "{err}");
    assert!(refused("code-review=").contains("no model after the ="));
    assert!(refused("code-review=codex-acp:").contains("no model after the `:`"));
    // Skills with no `=MODEL` at all are half a slot too, `@EFFORT` or not.
    let err = refused("code-review");
    assert!(err.contains("a model is required"), "{err}");
    let err = refused("Reviewer@high");
    assert!(err.contains("a model is required"), "{err}");
    // And the half after the `@`, which the forms in the refusal spell out.
    let err = refused("code-review=codex-acp:o3@");
    assert!(err.contains("no effort was named"), "{err}");
    assert!(refused("code-review=@high").contains("SKILLS=MODEL@EFFORT"));
}

/// A repository is a checkout and a base branch, and that is all it takes:
/// how a change reaches that base branch is the task's own `landing`, agreed
/// with the user task by task, so nothing about landing is registered here.
#[test]
fn a_repository_is_a_checkout_and_a_base_branch_and_says_nothing_about_landing() {
    let Command::Repo {
        command:
            RepoCommand::Add {
                path,
                branch,
                description,
            },
    } = parse(&[
        "ariadne",
        "repo",
        "add",
        "/tmp/repo",
        "--branch",
        "next",
        "--description",
        "the API",
    ])
    .command
    else {
        panic!("repo add");
    };
    assert_eq!(path, "/tmp/repo");
    assert_eq!(branch.as_deref(), Some("next"));
    assert_eq!(description.as_deref(), Some("the API"));

    // The flags that used to say how landing works are gone, not ignored.
    for gone in [
        &["--merge-strategy", "pull-request"][..],
        &["--landing-prompt", "Land it."][..],
        &["--landing-prompt-file", "brief.md"][..],
    ] {
        let mut argv = vec!["ariadne", "repo", "add", "/r"];
        argv.extend_from_slice(gone);
        assert!(try_parse(&argv).is_err(), "{gone:?} still parses");
    }
    for gone in [
        &["--merge-strategy", "direct"][..],
        &["--reset-landing-prompt"][..],
    ] {
        let mut argv = vec!["ariadne", "repo", "update", "01REPO"];
        argv.extend_from_slice(gone);
        assert!(try_parse(&argv).is_err(), "{gone:?} still parses");
    }
    assert!(
        try_parse(&["ariadne", "repo", "prompt", "get", "01REPO"]).is_err(),
        "repo prompt still parses"
    );
}

/// `--host` was the documented spelling before `--endpoint`; scripts that
/// still use it must land in the same field.
#[test]
fn the_old_host_flag_still_names_the_endpoint() {
    for flag in ["--endpoint", "--host"] {
        let cli = parse(&["ariadne", flag, "/tmp/x.sock", "version"]);
        assert_eq!(cli.endpoint.as_deref(), Some("/tmp/x.sock"), "{flag}");
    }
}

/// The flag keeps working where it is no longer advertised — hiding it is
/// about help text, not about breaking a command line that has it.
#[test]
fn a_hidden_format_flag_is_still_parsed() {
    assert_eq!(
        parse(&["ariadne", "attach", "--format", "json", "x"]).format,
        Format::Json
    );
    assert_eq!(
        parse(&["ariadne", "--format", "json", "attach", "x"]).format,
        Format::Json
    );
    assert_eq!(parse(&["ariadne", "attach", "x"]).format, Format::Table);
}

/// The flag list is replaced whole, so a line has to say how: name the
/// flags, clear them, or go back to the default — and never two of those.
#[test]
fn updating_an_agent_takes_flags_or_clear_or_reset_but_only_one() {
    let update = |args: &[&str]| {
        let mut argv = vec!["ariadne", "agent", "update", "claude-agent-acp"];
        argv.extend_from_slice(args);
        try_parse(&argv).is_ok()
    };
    assert!(!update(&[]), "nothing to do");
    assert!(update(&["--flag", "--verbose"]), "flags");
    assert!(update(&["--clear-flags"]), "--clear-flags");
    assert!(update(&["--reset"]), "--reset");
    assert!(
        !update(&["--flag", "--verbose", "--reset"]),
        "flags + reset"
    );
    assert!(!update(&["--flag", "--verbose", "--clear-flags"]), "both");
    assert!(!update(&["--clear-flags", "--reset"]), "clear + reset");
}

/// Every flag an agent takes starts with a dash, so a `--flag` value that
/// reads like a flag of clap's own has to reach the daemon as it was
/// typed — that is the whole point of the option.
#[test]
fn an_agent_flag_that_looks_like_a_flag_is_taken_as_it_is() {
    let Command::Agent {
        command: AgentCommand::Update { agent, flags, .. },
    } = parse(&[
        "ariadne",
        "agent",
        "update",
        "claude-agent-acp",
        "--flag",
        "--model-config",
        "--flag",
        "--verbose",
    ])
    .command
    else {
        panic!("agent update");
    };
    assert_eq!(agent, "claude-agent-acp");
    assert_eq!(flags, ["--model-config", "--verbose"]);
}

/// A skill is one document, so its lines are the four things one does to a
/// document plus the delete: `get` pipes it out, `set` writes it back from a
/// file or from stdin, `reset` puts a shipped one back, and each of them takes
/// the skill by name. `reset` and `rm` take the confirmation flag with them.
#[test]
fn a_skill_line_takes_the_skill_by_name() {
    let named = |command: SkillCommand| match command {
        SkillCommand::Inspect { name }
        | SkillCommand::Get { name }
        | SkillCommand::Create { name, .. }
        | SkillCommand::Set { name, .. }
        | SkillCommand::Reset { name, .. }
        | SkillCommand::Rm { name, .. } => name,
        SkillCommand::Ls => unreachable!("ls names no skill"),
    };
    for verb in ["inspect", "get", "reset", "rm"] {
        let Command::Skill { command } = parse(&["ariadne", "skill", verb, "coding"]).command
        else {
            panic!("skill {verb}");
        };
        assert_eq!(named(command), "coding");
    }

    // The document comes from a file, or from stdin where none is named.
    let Command::Skill {
        command: SkillCommand::Set { name, file },
    } = parse(&["ariadne", "skill", "set", "coding", "--file", "/tmp/c.md"]).command
    else {
        panic!("skill set");
    };
    assert_eq!(name, "coding");
    assert_eq!(file, Some(PathBuf::from("/tmp/c.md")));

    let Command::Skill {
        command: SkillCommand::Set { file, .. },
    } = parse(&["ariadne", "skill", "set", "coding"]).command
    else {
        panic!("skill set from stdin");
    };
    assert_eq!(file, None, "no file named is stdin");

    let Command::Skill {
        command: SkillCommand::Reset { yes, .. },
    } = parse(&["ariadne", "skill", "reset", "coding", "-y"]).command
    else {
        panic!("skill reset");
    };
    assert!(yes);

    let Command::Skill {
        command: SkillCommand::Rm { yes, .. },
    } = parse(&["ariadne", "skill", "rm", "mine", "--yes"]).command
    else {
        panic!("skill rm");
    };
    assert!(yes);
}

/// The daemon group is about one home, so `--home` is the group's: it reaches
/// every subcommand under it, wherever on the line it is typed.
#[test]
fn the_daemon_group_takes_one_home_for_all_of_it() {
    let home = |argv: &[&str]| {
        let Command::Daemon { home, .. } = parse(argv).command else {
            panic!("daemon");
        };
        home
    };
    let scratch = Some(PathBuf::from("/scratch"));
    assert_eq!(
        home(&["ariadne", "daemon", "--home", "/scratch", "stop"]),
        scratch
    );
    assert_eq!(
        home(&["ariadne", "daemon", "stop", "--home", "/scratch"]),
        scratch
    );
    assert_eq!(
        home(&["ariadne", "daemon", "--home", "/scratch", "start"]),
        scratch
    );
    assert_eq!(
        home(&["ariadne", "daemon", "restart", "--home", "/scratch"]),
        scratch
    );
    assert_eq!(
        home(&["ariadne", "daemon", "logs", "--home", "/scratch"]),
        scratch
    );
    assert_eq!(home(&["ariadne", "daemon", "status"]), None);
}

/// Stopping is over when the daemon is gone, so both commands that wait carry
/// the same bound on the wait — ten seconds unless the caller says otherwise.
#[test]
fn stopping_and_restarting_wait_for_a_bounded_time() {
    let timeout = |argv: &[&str]| {
        let Command::Daemon { command, .. } = parse(argv).command else {
            panic!("daemon");
        };
        match command {
            DaemonCommand::Stop { timeout } | DaemonCommand::Restart { timeout } => timeout,
            _ => panic!("stop or restart"),
        }
    };
    assert_eq!(timeout(&["ariadne", "daemon", "stop"]), STOP_TIMEOUT);
    assert_eq!(timeout(&["ariadne", "daemon", "restart"]), STOP_TIMEOUT);
    assert_eq!(
        timeout(&["ariadne", "daemon", "stop", "--timeout", "30"]),
        30
    );
    assert!(
        try_parse(&["ariadne", "daemon", "stop", "--timeout", "soon"]).is_err(),
        "a wait is a number of seconds"
    );
}

/// `session send` is the CLI's half of the UI's console: an id and the text
/// to send.
#[test]
fn session_send_takes_an_id_and_the_text_to_send() {
    let Command::Session {
        command: SessionCommand::Send { id, text },
    } = parse(&["ariadne", "session", "send", "01SESS", "make it green"]).command
    else {
        panic!("session send");
    };
    assert_eq!((id.as_str(), text.as_str()), ("01SESS", "make it green"));
    assert!(
        try_parse(&["ariadne", "session", "send", "01SESS"]).is_err(),
        "there is nothing to send"
    );
}

/// The attention filter is the daemon's own, and a flag that is not given is
/// no filter at all.
#[test]
fn session_ls_filters_on_attention() {
    let attention = |argv: &[&str]| {
        let Command::Session {
            command: SessionCommand::Ls { attention, .. },
        } = parse(argv).command
        else {
            panic!("session ls");
        };
        attention
    };
    assert!(attention(&["ariadne", "session", "ls", "--attention"]));
    assert!(!attention(&["ariadne", "session", "ls"]));
}

/// `models ls` narrows to an agent CLI in the spelling the daemon reads, and
/// the hyphenated one a shell tends to type names the same CLI.
#[test]
fn models_ls_takes_an_agent_to_narrow_the_catalogue() {
    let agent = |argv: &[&str]| {
        let Command::Models {
            command: ModelsCommand::Ls { agent, .. },
        } = parse(argv).command
        else {
            panic!("models ls");
        };
        agent
    };
    assert_eq!(agent(&["ariadne", "models", "ls"]), None);
    assert_eq!(
        agent(&["ariadne", "models", "ls", "--agent", "claude-agent-acp"]).as_deref(),
        Some("claude-agent-acp")
    );
}

/// `models show` takes a model in the same spelling `--model` does, and
/// refuses the same way `--model` would.
#[test]
fn models_show_takes_a_model_in_the_spelling_dash_dash_model_takes() {
    let model = |argv: &[&str]| {
        let Command::Models {
            command: ModelsCommand::Show { model },
        } = parse(argv).command
        else {
            panic!("models show");
        };
        model
    };
    assert_eq!(
        model(&["ariadne", "models", "show", "codex-acp:gpt-5.6-luna"]),
        "codex-acp:gpt-5.6-luna"
    );
    // The catalog has no bare-agent entry, so a bare agent is refused the
    // way `--model` refuses it.
    let Err(err) = try_parse(&["ariadne", "models", "show", "codex-acp"]) else {
        panic!("a bare agent names no model");
    };
    assert!(err.to_string().contains("names no agent"), "{err}");
}

fn parse(argv: &[&str]) -> Cli {
    Cli::from_arg_matches(&command().get_matches_from(argv)).expect("parse")
}

fn try_parse(argv: &[&str]) -> Result<Cli, clap::Error> {
    Cli::from_arg_matches(&command().try_get_matches_from(argv)?)
}

/// Every runnable command in the tree, sorted, as `"task ls"` —
/// hidden ones included, clap's generated `help` left out.
fn leaf_paths() -> Vec<String> {
    fn walk(cmd: &clap::Command, prefix: &str, out: &mut Vec<String>) {
        let mut leaf = true;
        for sub in cmd.get_subcommands().filter(|s| s.get_name() != "help") {
            leaf = false;
            let path = match prefix {
                "" => sub.get_name().to_string(),
                _ => format!("{prefix} {}", sub.get_name()),
            };
            walk(sub, &path, out);
        }
        // A command with subcommands is a grouping and not run on its own —
        // unless it takes an argument of its own too, as `completions
        // <SHELL>` does next to `completions install`.
        let runs_itself = leaf || cmd.get_positionals().next().is_some();
        if runs_itself && !prefix.is_empty() {
            out.push(prefix.to_string());
        }
    }
    let mut cmd = command();
    cmd.build();
    let mut out = Vec::new();
    walk(&cmd, "", &mut out);
    out.sort();
    out
}

/// One help screen as `--help` renders it (`[]` = `ariadne --help`).
fn long_help(path: &[&str]) -> String {
    let mut cmd = command();
    cmd.build();
    let mut screen = &mut cmd;
    for name in path {
        screen = screen.find_subcommand_mut(name).expect("subcommand");
    }
    screen.render_long_help().to_string()
}

/// The built command at `path` (`[]` = `ariadne` itself).
fn subcommand(path: &[&str]) -> clap::Command {
    let mut cmd = command();
    cmd.build();
    let mut found = &cmd;
    for name in path {
        found = found.find_subcommand(name).expect("subcommand");
    }
    found.clone()
}

/// The whole tree, built: globals only reach the subcommands once it is.
/// Building it is the expensive half, so a test that asks about every leaf
/// builds it once.
fn built() -> clap::Command {
    let mut cmd = command();
    cmd.build();
    cmd
}

/// Whether the global argument `id` shows up in that subcommand's help.
fn advertises(cmd: &clap::Command, path: &[&str], id: &str) -> bool {
    let mut sub = cmd;
    for name in path {
        sub = sub.find_subcommand(name).expect("subcommand");
    }
    sub.get_arguments()
        .any(|a| a.get_id() == id && !a.is_hide_set())
}
