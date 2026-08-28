//! The command tree as a conformance table: every leaf classified, and the
//! spellings scripts depend on pinned to the field they land in.

use super::*;

use clap::FromArgMatches;

use ariadne_core::{AgentKind, GoalStatus, MergeStrategy, Role, TaskStatus};

use crate::commands::profile::PromptAssignment;

/// clap's own consistency check over the whole tree, shadowed `--format`
/// arguments included.
#[test]
fn the_command_tree_is_well_formed() {
    command().debug_assert();
}

/// Every command a user can actually run, and whether `--format` shapes
/// what it prints. Hidden internal commands are in here too: `--format`
/// is global, so it reaches them whether or not anyone meant it to.
const LEAVES: &[(&str, bool)] = &[
    ("_spawn", false),
    ("agent list", true),
    ("agent update", true),
    ("agent-event", false),
    ("attach", false),
    ("attention", true),
    ("completions", false),
    ("daemon logs", false),
    ("daemon start", true),
    ("daemon status", true),
    ("daemon stop", true),
    ("doctor", true),
    ("goal attach", false),
    ("goal cancel", true),
    ("goal create", true),
    ("goal inspect", true),
    ("goal ls", true),
    ("goal messages", true),
    ("goal msg", true),
    ("goal rm", true),
    ("mcp serve", false),
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
    ("repo edit", true),
    ("repo inspect", true),
    ("repo ls", true),
    ("repo rm", true),
    ("session inspect", true),
    ("session kill", true),
    ("session logs", true),
    ("session ls", true),
    ("session resume", true),
    ("setup codex-hooks", false),
    ("task attach", false),
    ("task cancel", true),
    ("task create", true),
    ("task diff", true),
    ("task history", true),
    ("task inspect", true),
    ("task logs", true),
    ("task ls", true),
    ("task messages", true),
    ("task msg", true),
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
    for (path, honored) in LEAVES {
        let path: Vec<&str> = path.split(' ').collect();
        assert_eq!(
            advertises_format(&path),
            *honored,
            "--format is {} on {path:?}",
            if *honored { "missing" } else { "advertised" }
        );
    }
}

/// `_spawn` is tmux's end of a launch, not a command anyone types: it takes
/// the plan path and stays out of the help.
#[test]
fn the_spawn_command_takes_a_plan_path_and_is_hidden() {
    let Command::Spawn { plan } = parse(&["ariadne", "_spawn", "/run/s/spawn.json"]).command
    else {
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
        command: TaskCommand::Ls { status, .. },
    } = parse(&["ariadne", "task", "ls", "--status", "approved"]).command
    else {
        panic!("task ls");
    };
    assert_eq!(status, Some(TaskStatus::Approved));
    assert!(
        try_parse(&["ariadne", "task", "ls", "--status", "integrating"]).is_err(),
        "and the status a task was landed from by a fourth role is gone"
    );
}

/// `--to` is what addresses a message, on either thread, and leaving it out is
/// still the unaddressed post it has always been.
#[test]
fn a_message_is_addressed_only_when_to_says_so() {
    let to = |argv: &[&str]| match parse(argv).command {
        Command::Goal {
            command: GoalCommand::Msg { to, .. },
        } => to,
        Command::Task {
            command: TaskCommand::Msg { to, .. },
        } => to,
        _ => panic!("msg"),
    };
    assert_eq!(to(&["ariadne", "goal", "msg", "01GOAL", "ping"]), None);
    assert_eq!(
        to(&["ariadne", "goal", "msg", "01GOAL", "ping", "--to", "user"]).as_deref(),
        Some("user")
    );
    assert_eq!(to(&["ariadne", "task", "msg", "01TASK", "ping"]), None);
    assert_eq!(
        to(&["ariadne", "task", "msg", "01TASK", "ping", "--to", "Engineer"]).as_deref(),
        Some("Engineer")
    );
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
        command:
            TaskCommand::Create {
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

/// A model does not say which CLI runs it, so one that names no agent CLI is
/// refused on the line it was typed on — with the spelling that would have
/// named one, never a request the daemon has to turn down.
#[test]
fn a_model_naming_no_agent_is_a_usage_error() {
    let lines: [&[&str]; 3] = [
        &[
            "ariadne", "goal", "create", "--title", "Ship it", "--repo", "01REPO", "--model",
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
            "ariadne", "profile", "create", "--name", "eng", "--role", "engineer", "--model",
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
            "ariadne", "task", "create", "01GOAL", "--title", "Do it", "--reviewer", spec,
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
        command: RepoCommand::Edit { merge_strategy, .. },
    } = parse(&[
        "ariadne",
        "repo",
        "edit",
        "01REPO",
        "--merge-strategy",
        "direct",
    ])
    .command
    else {
        panic!("repo edit");
    };
    assert_eq!(merge_strategy, Some(MergeStrategy::Direct));
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
    assert!(err.contains("engineer_briefing"), "{err}");
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
        if leaf && !prefix.is_empty() {
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

/// Whether `--format` shows up in that subcommand's help.
fn advertises_format(path: &[&str]) -> bool {
    let mut cmd = command();
    // Globals only reach the subcommands once the tree is built.
    cmd.build();
    let mut sub = &cmd;
    for name in path {
        sub = sub.find_subcommand(name).expect("subcommand");
    }
    sub.get_arguments()
        .any(|a| a.get_id() == "format" && !a.is_hide_set())
}
