//! The command tree as a conformance table: every leaf classified, and the
//! spellings scripts depend on pinned to the field they land in.

use super::*;

use clap::FromArgMatches;

use ariadne_core::{AgentKind, GoalStatus, MergeStrategy, Role, SessionStatus, TaskStatus};

use crate::commands::models::ModelsCommand;
use crate::commands::profile::{PromptAssignment, PromptCommand};
use crate::commands::repo::RepoPromptCommand;
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
    "agent", "daemon", "goal", "profile", "repo", "session", "task",
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
    ("_spawn", false),
    ("agent ls", true),
    ("agent update", true),
    ("agent-event", false),
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
    ("goal create", true),
    ("goal inspect", true),
    ("goal ls", true),
    ("goal rm", true),
    ("mcp serve", false),
    ("models ls", true),
    ("models show", true),
    ("profile create", true),
    ("profile inspect", true),
    ("profile ls", true),
    ("profile prompt get", true),
    ("profile prompt reset", true),
    ("profile prompt set", true),
    ("profile prompts", true),
    ("profile rm", true),
    ("profile update", true),
    ("repo add", true),
    ("repo inspect", true),
    ("repo ls", true),
    ("repo prompt get", true),
    ("repo prompt reset", true),
    ("repo prompt set", true),
    ("repo rm", true),
    ("repo update", true),
    ("session inspect", true),
    ("session kill", true),
    ("session logs", true),
    ("session ls", true),
    ("session resume", true),
    ("session send", true),
    ("setup codex-hooks", false),
    ("task attach", false),
    ("task cancel", true),
    ("task create", true),
    ("task diff", true),
    ("task history", true),
    ("task inspect", true),
    ("task logs", true),
    ("task ls", true),
    ("task retry", true),
    ("task reviews", true),
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

/// The listing flags belong to the commands that print a table, and the
/// pager flag to the ones that print something long — advertised there and
/// nowhere else, so `ariadne task cancel --help` is still four lines.
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
        for id in ["quiet", "no_trunc", "layout", "columns"] {
            assert_eq!(advertises(&cmd, &path, id), listing, "{id} on {leaf:?}");
        }
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

/// `_spawn` is tmux's end of a launch, not a command anyone types: it takes
/// the plan path and stays out of the help.
#[test]
fn the_spawn_command_takes_a_plan_path_and_is_hidden() {
    let Command::Spawn { plan } = parse(&["ariadne", "_spawn", "/run/s/spawn.json"]).command else {
        panic!("_spawn");
    };
    assert_eq!(plan, PathBuf::from("/run/s/spawn.json"));
    assert!(
        try_parse(&["ariadne", "_spawn"]).is_err(),
        "a launch with no plan is a usage error"
    );
    let mut cmd = command();
    cmd.build();
    assert!(
        cmd.find_subcommand("_spawn").expect("_spawn").is_hide_set(),
        "_spawn is advertised in the help"
    );
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
        command: SessionCommand::Ls { role, .. },
    } = parse(&["ariadne", "session", "ls", "--role", "reviewer"]).command
    else {
        panic!("session ls");
    };
    assert_eq!(role, Some(Role::Reviewer));
    assert!(
        try_parse(&["ariadne", "session", "ls", "--role", "critic"]).is_err(),
        "an unknown role is a usage error"
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
        "and the status a task was landed from by a fourth role is gone"
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
    let Command::Repo {
        command: RepoCommand::Add { merge_strategy, .. },
    } = parse(&[
        "ariadne",
        "repo",
        "add",
        "/r",
        "--merge-strategy",
        "pull-request",
    ])
    .command
    else {
        panic!("repo add");
    };
    assert_eq!(
        merge_strategy,
        MergeStrategy::PullRequest,
        "and so is every other enum a flag takes"
    );
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

/// Prompt kinds go the same way, on the argument as on the flag: they are
/// parsed by hand rather than by clap, and the hand-written parser has to
/// take the two spellings too.
#[test]
fn a_prompt_kind_is_spelled_in_kebab_or_in_snake() {
    let got = |spelling: &str| {
        let Command::Profile {
            command:
                ProfileCommand::Prompt {
                    command: PromptCommand::Get { kind, .. },
                },
        } = parse(&["ariadne", "profile", "prompt", "get", "Engineer", spelling]).command
        else {
            panic!("profile prompt get");
        };
        kind
    };
    assert_eq!(got("engineer-briefing"), got("engineer_briefing"));
    assert_eq!(got("engineer-briefing").as_str(), "engineer_briefing");
    assert_eq!(got("system").as_str(), "system");

    let (texts, _) = create_flags(&["--prompt", "changes-requested=Fix it"]);
    assert_eq!(pairs(texts), ["changes_requested=Fix it"]);
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
/// for the planner and the engineer, `--reviewer PROFILE=MODEL` per reviewer
/// slot, and every spelling lands in the field the request is built from.
#[test]
fn a_model_can_be_chosen_for_every_agent_on_the_line() {
    let planner = |args: &[&str]| {
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
        planner(&["--model", "codex:gpt-5.3-codex"]).as_deref(),
        Some("codex:gpt-5.3-codex")
    );
    assert_eq!(
        planner(&["--model", "codex"]).as_deref(),
        Some("codex"),
        "an agent CLI on its own runs it on its own default model"
    );
    assert_eq!(
        planner(&[]),
        None,
        "and nothing at all is the planner profile's own"
    );
    assert_eq!(
        planner(&["--model", "claude-code"]).as_deref(),
        Some("claude_code"),
        "the hyphenated spelling names the same CLI, and travels as the daemon \
         spells it"
    );

    let Command::Task {
        command: TaskCommand::Create {
            model, reviewers, ..
        },
    } = parse(&[
        "ariadne",
        "task",
        "create",
        "01GOAL",
        "--title",
        "Do it",
        "--model",
        "claude_code:claude-opus-5",
        "--reviewer",
        "Reviewer=codex:o3",
        "--reviewer",
        "rev-strict=opencode",
        "--reviewer",
        "Security",
    ])
    .command
    else {
        panic!("task create")
    };
    assert_eq!(model.as_deref(), Some("claude_code:claude-opus-5"));
    assert_eq!(
        reviewers
            .iter()
            .map(|r| (r.profile.as_str(), r.model.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("Reviewer", Some("codex:o3")),
            ("rev-strict", Some("opencode")),
            ("Security", None),
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
        edited(&["--model", "default"]).as_deref(),
        Some("default"),
        "\"default\" hands the task back to its engineer profile's pin"
    );
    assert_eq!(
        edited(&["--model", "claude-code"]).as_deref(),
        Some("claude_code"),
        "and a CLI travels in the spelling the daemon reads"
    );
    assert_eq!(
        edited(&["--model", "codex:gpt-5.3-codex"]).as_deref(),
        Some("codex:gpt-5.3-codex")
    );
    assert_eq!(edited(&["--title", "Do it better"]), None);
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
        "codex:gpt-5.6-sol",
        "--effort",
        "xhigh",
    ])
    .command
    else {
        panic!("goal create")
    };
    assert_eq!(model.as_deref(), Some("codex:gpt-5.6-sol"));
    assert_eq!(effort.as_deref(), Some("xhigh"));

    let Command::Profile {
        command: ProfileCommand::Create { effort, .. },
    } = parse(&[
        "ariadne",
        "profile",
        "create",
        "--name",
        "eng",
        "--role",
        "engineer",
        "--model",
        "claude_code:claude-opus-5",
        "--effort",
        "max",
    ])
    .command
    else {
        panic!("profile create")
    };
    assert_eq!(effort.as_deref(), Some("max"));

    let Command::Profile {
        command: ProfileCommand::Update { effort, .. },
    } = parse(&[
        "ariadne", "profile", "update", "Reviewer", "--effort", "default",
    ])
    .command
    else {
        panic!("profile update")
    };
    assert_eq!(
        effort.as_deref(),
        Some("default"),
        "\"default\" runs the model at whatever its agent CLI runs it at"
    );

    let Command::Task {
        command: TaskCommand::Create {
            effort, reviewers, ..
        },
    } = parse(&[
        "ariadne",
        "task",
        "create",
        "01GOAL",
        "--title",
        "Do it",
        "--effort",
        "xhigh",
        "--reviewer",
        "Reviewer=codex:gpt-5.6-sol@xhigh",
        "--reviewer",
        "rev-strict@high",
        "--reviewer",
        "Security=codex",
    ])
    .command
    else {
        panic!("task create")
    };
    assert_eq!(effort.as_deref(), Some("xhigh"));
    assert_eq!(
        reviewers
            .iter()
            .map(|r| (r.profile.as_str(), r.model.as_deref(), r.effort.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("Reviewer", Some("codex:gpt-5.6-sol"), Some("xhigh")),
            ("rev-strict", None, Some("high")),
            ("Security", Some("codex"), None),
        ],
        "a slot says a model, an effort, or both — and neither is guessed from \
         the other"
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
    assert_eq!(edited(&["--model", "codex"]), None);
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
    // effort no claude model runs at is still sent, and refused there.
    assert!(
        try_parse(&[
            "ariadne",
            "task",
            "update",
            "01TASK",
            "--model",
            "claude_code:claude-opus-5",
            "--effort",
            "ultra",
        ])
        .is_ok()
    );
}

/// A model does not say which CLI runs it, so one that names no agent CLI is
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
            "--model",
            "gpt-5.3-codex",
        ],
        &[
            "ariadne",
            "profile",
            "create",
            "--name",
            "eng",
            "--role",
            "engineer",
            "--model",
            "gpt-5.3-codex",
        ],
    ];
    for argv in lines {
        let err = try_parse(argv)
            .map(|_| ())
            .expect_err("a model naming no agent")
            .to_string();
        assert!(err.contains("names no agent CLI"), "{argv:?}: {err}");
        assert!(err.contains("claude_code:gpt-5.3-codex"), "{argv:?}: {err}");
    }

    // `default` is a word only an update takes: on a create it names no CLI
    // either, since nothing is being handed back.
    assert!(
        try_parse(&[
            "ariadne", "goal", "create", "--title", "Ship it", "--repo", "01REPO", "--model",
            "default",
        ])
        .is_err()
    );
}

/// `task update --model` takes a model or the word "default" and nothing else:
/// an agent CLI Ariadne does not run is refused on the line it was typed on,
/// never sent for the daemon to turn down.
#[test]
fn a_model_on_an_agent_that_is_no_cli_is_a_usage_error() {
    let err = try_parse(&["ariadne", "task", "update", "01TASK", "--model", "llama:x"])
        .map(|_| ())
        .expect_err("no such agent")
        .to_string();
    assert!(err.contains("unknown agent `llama`"), "{err}");
    assert!(err.contains("claude_code, codex, opencode"), "{err}");
    assert!(err.contains("default"), "{err}");
}

/// A `--reviewer` that says half of what it means is a typo, and it is
/// refused where it was typed rather than sent to the daemon to be refused
/// there — with the form it accepts and the agent CLIs that stand in it.
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
    let err = refused("Reviewer=llama");
    assert!(err.contains("names no agent CLI"), "{err}");
    assert!(err.contains("claude_code, codex, opencode"), "{err}");
    assert!(refused("Reviewer=").contains("no model after the ="));
    assert!(refused("Reviewer=codex:").contains("no model after the `:`"));
    // And the half after the `@`, which the forms in the refusal spell out.
    let err = refused("Reviewer@");
    assert!(err.contains("no effort was named"), "{err}");
    assert!(refused("Reviewer=@high").contains("PROFILE=MODEL@EFFORT"));
}

/// How a repository takes a change is the user's to set, on the way in
/// and afterwards; a repository nobody said anything about is landed on
/// directly.
#[test]
fn a_repository_can_be_registered_with_a_merge_strategy() {
    let added = |args: &[&str]| {
        let mut argv = vec!["ariadne", "repo", "add", "/tmp/repo"];
        argv.extend_from_slice(args);
        let Command::Repo {
            command: RepoCommand::Add { merge_strategy, .. },
        } = parse(&argv).command
        else {
            panic!("repo add");
        };
        merge_strategy
    };
    assert_eq!(added(&[]), MergeStrategy::Direct);
    assert_eq!(
        added(&["--merge-strategy", "pull_request"]),
        MergeStrategy::PullRequest
    );
    assert!(
        try_parse(&["ariadne", "repo", "add", "/r", "--merge-strategy", "forge"]).is_err(),
        "an unknown strategy is a usage error"
    );

    let Command::Repo {
        command: RepoCommand::Update { merge_strategy, .. },
    } = parse(&[
        "ariadne",
        "repo",
        "update",
        "01REPO",
        "--merge-strategy",
        "direct",
    ])
    .command
    else {
        panic!("repo update");
    };
    assert_eq!(merge_strategy, Some(MergeStrategy::Direct));
}

/// The landing briefing is set from text on the line, a file, or — on
/// `update` only — reset to the merge strategy's default; never two of those
/// at once.
#[test]
fn a_landing_prompt_is_set_from_text_a_file_or_reset_but_never_two_of_those() {
    let Command::Repo {
        command:
            RepoCommand::Add {
                landing_prompt,
                landing_prompt_file,
                ..
            },
    } = parse(&[
        "ariadne",
        "repo",
        "add",
        "/r",
        "--landing-prompt",
        "Land it.",
    ])
    .command
    else {
        panic!("repo add");
    };
    assert_eq!(landing_prompt.as_deref(), Some("Land it."));
    assert_eq!(landing_prompt_file, None);

    let Command::Repo {
        command: RepoCommand::Add {
            landing_prompt_file,
            ..
        },
    } = parse(&[
        "ariadne",
        "repo",
        "add",
        "/r",
        "--landing-prompt-file",
        "brief.md",
    ])
    .command
    else {
        panic!("repo add");
    };
    assert_eq!(landing_prompt_file, Some(PathBuf::from("brief.md")));

    assert!(
        try_parse(&[
            "ariadne",
            "repo",
            "add",
            "/r",
            "--landing-prompt",
            "x",
            "--landing-prompt-file",
            "brief.md",
        ])
        .is_err(),
        "text and a file at once is a usage error"
    );

    let update = |args: &[&str]| {
        let mut argv = vec!["ariadne", "repo", "update", "01REPO"];
        argv.extend_from_slice(args);
        try_parse(&argv).is_ok()
    };
    assert!(update(&[]), "nothing to say leaves it unchanged");
    assert!(update(&["--landing-prompt", "Land it."]), "text");
    assert!(update(&["--landing-prompt-file", "brief.md"]), "a file");
    assert!(update(&["--reset-landing-prompt"]), "reset");
    assert!(
        !update(&["--landing-prompt", "x", "--reset-landing-prompt"]),
        "text + reset"
    );
    assert!(
        !update(&[
            "--landing-prompt-file",
            "brief.md",
            "--reset-landing-prompt"
        ]),
        "file + reset"
    );
    assert!(
        !update(&["--landing-prompt", "x", "--landing-prompt-file", "brief.md"]),
        "text + file"
    );
}

/// `repo prompt` is the other half of the landing briefing story: it prints,
/// pipes and resets what `repo add`/`repo update` write.
#[test]
fn repo_prompt_gets_sets_and_resets_the_landing_briefing() {
    let Command::Repo {
        command: RepoCommand::Prompt {
            command: RepoPromptCommand::Get { id },
        },
    } = parse(&["ariadne", "repo", "prompt", "get", "01REPO"]).command
    else {
        panic!("repo prompt get");
    };
    assert_eq!(id, "01REPO");

    let Command::Repo {
        command:
            RepoCommand::Prompt {
                command: RepoPromptCommand::Set { id, file },
            },
    } = parse(&[
        "ariadne", "repo", "prompt", "set", "01REPO", "--file", "brief.md",
    ])
    .command
    else {
        panic!("repo prompt set");
    };
    assert_eq!(id, "01REPO");
    assert_eq!(file, Some(PathBuf::from("brief.md")));

    let Command::Repo {
        command:
            RepoCommand::Prompt {
                command: RepoPromptCommand::Reset { id, yes },
            },
    } = parse(&["ariadne", "repo", "prompt", "reset", "01REPO", "-y"]).command
    else {
        panic!("repo prompt reset");
    };
    assert_eq!(id, "01REPO");
    assert!(yes);
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
        let mut argv = vec!["ariadne", "agent", "update", "claude_code"];
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
        command: AgentCommand::Update { kind, flags, .. },
    } = parse(&[
        "ariadne",
        "agent",
        "update",
        "claude-code",
        "--flag",
        "--dangerously-skip-permissions",
        "--flag",
        "--verbose",
    ])
    .command
    else {
        panic!("agent update");
    };
    // The dash spelling names the agent the daemon calls claude_code.
    assert_eq!(kind, AgentKind::ClaudeCode);
    assert_eq!(flags, ["--dangerously-skip-permissions", "--verbose"]);
}

/// `profile prompt reset` has to be told what to reset: one kind, or
/// `--all`, and never both at once.
#[test]
fn resetting_a_prompt_takes_a_kind_or_all_but_not_both() {
    let reset = |args: &[&str]| {
        let mut argv = vec!["ariadne", "profile", "prompt", "reset", "Engineer"];
        argv.extend_from_slice(args);
        try_parse(&argv).is_ok()
    };
    assert!(!reset(&[]), "neither");
    assert!(reset(&["system"]), "a kind");
    assert!(reset(&["--all"]), "--all");
    assert!(!reset(&["system", "--all"]), "both");
}

/// The prompt flags name the kind they set, so one line can seed the
/// system prompt and a briefing at once, from text and from a file — on
/// `profile update` exactly as on `profile create`. A kind no role owns
/// never reaches the daemon, on either flag.
#[test]
fn a_prompt_flag_names_the_kind_it_sets() {
    let (texts, files) = create_flags(&[
        "--prompt",
        "system=You are...",
        "--prompt",
        "changes_requested=Fix it",
        "--prompt-file",
        "engineer_briefing=/tmp/b.md",
    ]);
    assert_eq!(
        pairs(texts),
        ["system=You are...", "changes_requested=Fix it"]
    );
    assert_eq!(pairs(files), ["engineer_briefing=/tmp/b.md"]);

    let Command::Profile {
        command:
            ProfileCommand::Update {
                prompts,
                prompt_files,
                ..
            },
    } = parse(&[
        "ariadne",
        "profile",
        "update",
        "Engineer",
        "--prompt",
        "system=You are...",
        "--prompt-file",
        "changes_requested=/tmp/c.md",
    ])
    .command
    else {
        panic!("profile update");
    };
    assert_eq!(pairs(prompts), ["system=You are..."]);
    assert_eq!(pairs(prompt_files), ["changes_requested=/tmp/c.md"]);

    for flag in ["--prompt", "--prompt-file"] {
        let err = try_create(&[flag, "briefing=x"])
            .expect_err("unknown kind")
            .to_string();
        assert!(err.contains("unknown prompt kind: briefing"), "{err}");
        assert!(err.contains("system"), "{err}");
    }
}

/// The old bare `--prompt <text>` is gone: a value with no kind is a
/// usage error, and the message says how to spell what was meant.
#[test]
fn a_prompt_without_a_kind_is_a_usage_error() {
    let err = try_create(&["--prompt", "You are..."])
        .expect_err("no kind")
        .to_string();
    assert!(err.contains("missing <kind>="), "{err}");
    assert!(err.contains("write system=<text>"), "{err}");
    assert!(err.contains("engineer-briefing"), "{err}");
    let err = try_create(&["--prompt-file", "/tmp/b.md"])
        .expect_err("no kind")
        .to_string();
    assert!(err.contains("write system=<path>"), "{err}");
}

/// The prompt flags of a `profile create` line, as clap parsed them.
fn create_flags(args: &[&str]) -> (Vec<PromptAssignment>, Vec<PromptAssignment>) {
    let Command::Profile {
        command:
            ProfileCommand::Create {
                prompts,
                prompt_files,
                ..
            },
    } = parse(&create_argv(args)).command
    else {
        panic!("profile create");
    };
    (prompts, prompt_files)
}

fn try_create(args: &[&str]) -> Result<(), clap::Error> {
    try_parse(&create_argv(args)).map(|_| ())
}

/// A `profile create` line with everything it needs, plus `args`.
fn create_argv<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut argv = vec![
        "ariadne", "profile", "create", "--name", "X", "--role", "engineer",
    ];
    argv.extend_from_slice(args);
    argv
}

/// Prompt flags back as `<kind>=<value>`, the way they were typed.
fn pairs(args: Vec<PromptAssignment>) -> Vec<String> {
    args.into_iter()
        .map(|a| format!("{}={}", a.kind.as_str(), a.value))
        .collect()
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

/// `session send` is the CLI's half of the UI's terminal panel: an id, the
/// text, and the one thing a caller may want differently — leaving it in the
/// prompt instead of submitting it.
#[test]
fn session_send_takes_an_id_and_the_text_to_type() {
    let Command::Session {
        command:
            SessionCommand::Send {
                id,
                text,
                no_newline,
            },
    } = parse(&["ariadne", "session", "send", "01SESS", "make it green"]).command
    else {
        panic!("session send");
    };
    assert_eq!(
        (id.as_str(), text.as_str(), no_newline),
        ("01SESS", "make it green", false)
    );

    let Command::Session {
        command: SessionCommand::Send { no_newline, .. },
    } = parse(&["ariadne", "session", "send", "01SESS", "y", "--no-newline"]).command
    else {
        panic!("session send");
    };
    assert!(no_newline);
    assert!(
        try_parse(&["ariadne", "session", "send", "01SESS"]).is_err(),
        "there is nothing to type"
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
        agent(&["ariadne", "models", "ls", "--agent", "claude_code"]),
        Some(AgentKind::ClaudeCode)
    );
    assert_eq!(
        agent(&["ariadne", "models", "ls", "--agent", "claude-code"]),
        Some(AgentKind::ClaudeCode)
    );
    let Err(err) = try_parse(&["ariadne", "models", "ls", "--agent", "gemini"]) else {
        panic!("\"gemini\" is not an agent CLI");
    };
    let err = err.to_string();
    assert!(err.contains("unknown agent kind: gemini"), "{err}");
    assert!(
        err.contains("opencode"),
        "the refusal lists the real ones: {err}"
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
        model(&["ariadne", "models", "show", "codex:gpt-5.6-luna"]),
        "codex:gpt-5.6-luna"
    );
    assert_eq!(model(&["ariadne", "models", "show", "codex"]), "codex");
    let Err(err) = try_parse(&["ariadne", "models", "show", "gemini:nope"]) else {
        panic!("\"gemini\" is not an agent CLI");
    };
    assert!(err.to_string().contains("unknown agent `gemini`"));
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
