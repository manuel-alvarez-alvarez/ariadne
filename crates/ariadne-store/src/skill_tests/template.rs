use super::*;

/// The catalog is the whole of what an agent can be, so it is read here
/// as a catalog: every skill is named once, its name is the one its own
/// frontmatter gives, and every skill says in one line what it is for.
#[test]
fn every_shipped_skill_is_named_once_and_describes_itself() {
    let mut names: Vec<&str> = BUILTIN_SKILLS.iter().map(|s| s.name).collect();
    let listed = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), listed, "a skill is listed twice");

    for skill in &BUILTIN_SKILLS {
        assert!(
            skill
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-'),
            "{} is not kebab-case",
            skill.name
        );
        let front = skill
            .document
            .strip_prefix("---\n")
            .and_then(|rest| rest.split_once("\n---"))
            .map(|(front, _)| front)
            .unwrap_or_else(|| panic!("{} has no frontmatter", skill.name));
        assert!(
            front.contains(&format!("name: {}", skill.name)),
            "{} names itself something else in its frontmatter",
            skill.name
        );
        let summary = skill_summary(skill.document)
            .unwrap_or_else(|| panic!("{} describes itself nowhere", skill.name));
        assert!(
            !summary.is_empty() && summary.len() < 160,
            "the {} summary is {} characters; the index carries one line",
            skill.name,
            summary.len()
        );
    }
}

/// A background poll of a check was the largest single waste a
/// measurement of one week's sessions found: an agent started
/// `cargo nextest` in the background and re-sent its whole context on
/// every no-op turn spent waiting for it. The fix — a check in the
/// foreground, never polled, its output in a log outside the worktree —
/// is the session rules' to state (013), which every seat reads. The
/// skills that run checks say only which checks to run.
#[test]
fn the_skills_leave_how_a_check_runs_to_the_session_rules() {
    for (name, document) in all_skills() {
        let doc = unwrapped(document);
        for block in [
            "in the foreground",
            "no-op command",
            "outside the worktree",
            "tail -n 40",
            "the detail of a failure",
        ] {
            assert!(
                !doc.contains(block),
                "{name} repeats the check block: {block}"
            );
        }
    }
}

/// Never polling a request is the session rules' to state too (006 rule
/// 5), for the `agent` and `reviewer` seats alone: no shipped skill
/// names a timer or a poll of its own.
#[test]
fn no_skill_names_a_timer_or_a_poll() {
    for (name, document) in all_skills() {
        let words: Vec<String> = document
            .split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::to_lowercase)
            .collect();
        for gone in ["timer", "poll", "polling"] {
            assert!(
                !words.iter().any(|word| word == gone),
                "the {name} names a {gone}"
            );
        }
    }
}

/// One cap holds every skill, and one total holds the catalog (006 rule 13).
///
/// A skill is read whole, once per launch and again after a compaction,
/// so its cost is its own and not every turn's. 8,000 characters is about
/// 2,000 tokens: the largest skill today, `code-review`, is under it with
/// room for a rewrite, and it is well under the 500 lines and 5,000 tokens
/// the outside guidance holds a skill to. One number per skill, rather than
/// a budget per skill, lets a skill grow for a reason of its own without a
/// change here first. The total still bounds the catalog: 70,000 is
/// eleven skills at about 6,300 each, so no one skill can take the
/// whole room. The description cap belongs to the catalog test above.
#[test]
fn skill_size_caps_hold() {
    const CAP: usize = 8_000;
    const TOTAL: usize = 70_000;

    let read = |skill: &BuiltinSkill| skill_text(skill.document).len();
    for skill in &BUILTIN_SKILLS {
        println!("{:5}  {}", read(skill), skill.name);
    }
    for skill in &BUILTIN_SKILLS {
        assert!(
            read(skill) <= CAP,
            "the {} skill is {} characters, over {CAP}",
            skill.name,
            read(skill),
        );
    }
    let total: usize = BUILTIN_SKILLS.iter().map(read).sum();
    assert!(
        total <= TOTAL,
        "the skills total {total} characters, over {TOTAL}"
    );
}

#[test]
fn every_skill_text_is_simplified_technical_english() {
    for (name, text) in all_skills() {
        let text = ste::unwrapped_lines(text);
        for sentence in ste::sentences(&text) {
            let words = sentence.split_whitespace().count();
            assert!(
                words <= ste::MAX_WORDS,
                "the {name} runs a sentence of {words} words, over {}: {sentence}",
                ste::MAX_WORDS
            );
        }
        assert_eq!(
            ste::banned_word(&text),
            None,
            "the {name} uses a word STE has no room for"
        );
    }
}

#[test]
fn no_skill_repeats_what_every_session_is_told_by_the_mcp_server() {
    for (name, text) in all_skills() {
        for shared in [
            "Reach Ariadne",
            "backticked",
            "Work alone",
            "for a result, a question or a blocker",
            "as few turns as you can",
            "ASD-STE100",
            "before you repeat a discovery",
        ] {
            assert!(
                !text.contains(shared),
                "the {name} repeats \"{shared}\", which the MCP instructions state"
            );
        }
    }
}

#[test]
fn every_skill_obeys_the_template() {
    assert_eq!(BUILTIN_SKILLS.len(), 11);
    assert_eq!(
        MERGED_SKILLS,
        [
            ("testing", "coding"),
            ("refactoring", "coding"),
            ("spec-review", "code-review"),
            ("performance-review", "code-review")
        ]
    );
    for skill in &BUILTIN_SKILLS {
        let document = skill.document;
        let (_, body) = document
            .strip_prefix("---\n")
            .unwrap()
            .split_once("\n---\n")
            .unwrap();
        // The description is two sentences: what the skill does, then
        // "Use when" and its triggers. The orchestrator staffs from this
        // line and a native tool triggers on it, so both readers get both.
        let description = skill_summary(document).unwrap();
        let (does, triggers) = description
            .split_once(". ")
            .unwrap_or_else(|| panic!("{}: description is one sentence", skill.name));
        assert!(
            !does.is_empty() && !does.starts_with("Use when"),
            "{}: description opens with what the skill does",
            skill.name
        );
        assert!(
            triggers.starts_with("Use when ") && triggers.ends_with('.'),
            "{}: description ends with its triggers",
            skill.name
        );
        assert_eq!(
            description.matches('.').count(),
            2,
            "{}: two sentences",
            skill.name
        );
        assert!(
            !description.contains(" you ") && !description.starts_with("You "),
            "{}: third person",
            skill.name
        );
        let headings: Vec<_> = body
            .lines()
            .filter(|line| line.starts_with("## "))
            .collect();
        // No `## Done`: each step carries its own `Done when`, and a closing
        // section that restated them drifted from them.
        let required = [
            "## When not to use",
            "## Steps",
            "## Never",
            "## Do not tell yourself",
        ];
        let actual: Vec<_> = headings
            .iter()
            .copied()
            .filter(|line| *line != "## Rules")
            .collect();
        assert_eq!(actual, required, "{}: section order", skill.name);
        if let Some(rules) = headings.iter().position(|line| *line == "## Rules") {
            assert_eq!(rules, 2, "{}: rules follow steps", skill.name);
            let content = body
                .split_once("\n## Rules\n")
                .unwrap()
                .1
                .split("\n## ")
                .next()
                .unwrap();
            assert!(
                !content.trim().is_empty(),
                "{}: empty Rules section",
                skill.name
            );
        }
        let exclusions = body
            .split_once("## When not to use\n")
            .unwrap()
            .1
            .split("\n## ")
            .next()
            .unwrap();
        assert!(
            BUILTIN_SKILLS
                .iter()
                .any(|sibling| sibling.name != skill.name
                    && exclusions.contains(&format!("`{}`", sibling.name))),
            "{}: sibling",
            skill.name
        );
        // The outcome line is one sentence of purpose, with no end
        // condition of its own: the last step's `Done when` ends the skill.
        let mut lines = body.lines().filter(|line| !line.is_empty());
        assert!(lines.next().unwrap().starts_with("# "));
        let outcome = lines.next().unwrap();
        assert!(
            !outcome.starts_with('#')
                && ste::sentences(outcome).len() == 1
                && !outcome.contains("Done when"),
            "{}: outcome is one sentence of purpose: {outcome}",
            skill.name
        );
        assert!(body.lines().count() < 500, "{}: body line cap", skill.name);
        let steps = body
            .split_once("## Steps\n")
            .unwrap()
            .1
            .split("\n## ")
            .next()
            .unwrap();
        let mut count = 0;
        for line in steps.lines() {
            if line
                .split_once(". ")
                .is_some_and(|(prefix, _)| prefix.parse::<usize>().is_ok())
            {
                count += 1;
            }
        }
        assert!(count > 0, "{}: numbered steps", skill.name);
        // Each primary step has a completion boundary before the next step.
        let primary = steps.split("\n### ").next().unwrap();
        let mut bounded = true;
        for line in primary.lines() {
            if line
                .split_once(". ")
                .is_some_and(|(prefix, _)| prefix.parse::<usize>().is_ok())
            {
                assert!(bounded, "{}: unbounded step", skill.name);
                bounded = false;
            }
            if line.contains("Done when") {
                bounded = true;
            }
        }
        assert!(bounded, "{}: unbounded final step", skill.name);
        for retired in [
            "`request_review`",
            "`submit_verdict`",
            "`pick_winner`",
            "`finish_task`",
            "`complete_task`",
            "`fail_review`",
            "`approve_task`",
            "gh pr merge",
        ] {
            assert!(
                !document.contains(retired),
                "{}: retired call {retired}",
                skill.name
            );
        }
        let path = manifest_dir()
            .join("skills")
            .join(skill.name)
            .join("SKILL.md");
        let read =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(read, document);
    }
    for (old, new) in MERGED_SKILLS {
        assert!(default_skill_document(old).is_none());
        assert!(default_skill_document(new).is_some());
        assert!(!manifest_dir().join("skills").join(old).exists());
    }
}

#[test]
fn each_rule_is_stated_in_exactly_one_briefing() {
    // The step calls, the hook rule and the stop rule belong to the
    // agent seat text. `finalize_plan` belongs to the orchestration
    // skill. The worktree scope and the plan-with-the-user rule belong
    // to the agent and orchestrator seat texts: a skill that repeated
    // either, as `merge` and `orchestration` once did, is the mistake
    // this guards against.
    for marker in [
        "Call `complete_step` when",
        "Call `fail_step` when",
        "Never pass `--no-verify` or `-n` to git",
        "After three failed attempts at one fix, stop.",
        "Call it no earlier.",
        "Work only in the task's worktree",
        "Plan the goal with the user",
        "return the work with `fail_step`",
        "A skill uses plain words.",
    ] {
        let places = all_defaults()
            .into_iter()
            .chain(all_skills())
            .chain(all_supplements())
            .filter(|(_, text)| text.contains(marker))
            .map(|(name, _)| name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            places.len(),
            1,
            "\"{marker}\" is stated in {places:?}, not in one place"
        );
    }
    for (name, phrase) in [
        ("merge", "Work from your worktree"),
        ("orchestration", "Plan with the user"),
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        assert!(
            !document.contains(phrase),
            "the {name} skill repeats its seat's own \"{phrase}\""
        );
    }
}

/// The crate's directory, as the test runner sets it for this run. Not
/// `env!("CARGO_MANIFEST_DIR")`: a test binary built in another checkout
/// that shares the target directory, such as the pre-push hook's temporary
/// one, keeps that checkout's path, which may no longer exist.
fn manifest_dir() -> std::path::PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .expect("the test runner sets CARGO_MANIFEST_DIR")
        .into()
}

/// The word list (006 rule 21): each Ariadne word, and the plain word a
/// shipped skill says in its place. `None` is a word no skill uses at all.
/// The agent seat text binds each plain word back to its Ariadne word.
pub(super) const WORD_LIST: [(&str, Option<&str>); 9] = [
    ("orchestrator", Some("the requester")),
    ("column", Some("stage")),
    ("column report", Some("the report")),
    ("fail the step", Some("return the work")),
    ("news", Some("an update")),
    ("briefing", Some("your instructions")),
    ("`Repo:`", Some("the primary checkout of the repository")),
    ("seat", None),
    ("keeper", None),
];

/// Every tool the Ariadne MCP server lists, for any seat (013).
const MCP_TOOLS: [&str; 25] = [
    "get_task",
    "complete_step",
    "fail_step",
    "create_task",
    "update_task",
    "list_models",
    "list_hosts",
    "list_skills",
    "finalize_plan",
    "list_tasks",
    "retry_task",
    "cancel_task",
    "switch_session",
    "complete_goal",
    "fail_task",
    "open_pull_request",
    "get_diff",
    "get_pull_request",
    "list_comments",
    "get_comment",
    "reply_comment",
    "escalate_comment",
    "resolve_thread",
    "submit_review",
    "post_to_session",
];

/// What of Ariadne a skill `document` names: `Ariadne` itself, a tool of
/// its MCP server, or an Ariadne word of [`WORD_LIST`], each whatever its
/// case and in the plural too.
fn ariadne_words_in(document: &str) -> Vec<String> {
    let lowered = unwrapped(document).to_lowercase();
    let words: Vec<&str> = lowered
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .collect();
    let mut found: Vec<String> = Vec::new();
    for single in std::iter::once("ariadne").chain(MCP_TOOLS).chain(
        WORD_LIST
            .iter()
            .map(|(word, _)| *word)
            .filter(|w| w.chars().all(|c| c.is_ascii_alphanumeric())),
    ) {
        if words
            .iter()
            .any(|word| *word == single || word.strip_suffix('s') == Some(single))
        {
            found.push(single.to_string());
        }
    }
    for (phrase, _) in WORD_LIST {
        let phrase = phrase.to_lowercase();
        if !phrase.chars().all(|c| c.is_ascii_alphanumeric()) && lowered.contains(&phrase) {
            found.push(phrase);
        }
    }
    found
}

/// Fails where the skill `name`'s `document` names Ariadne, a tool of its
/// MCP server, or an Ariadne word of [`WORD_LIST`].
pub(super) fn assert_names_no_ariadne_word(name: &str, document: &str) {
    let found = ariadne_words_in(document);
    assert!(
        found.is_empty(),
        "the {name} skill names Ariadne words: {found:?}"
    );
}

/// `code-review` and `architecture-review` read the same in any harness:
/// they name no Ariadne, no MCP tool and no Ariadne word, and the agent seat
/// text and the supplements say what their plain words stand for (006
/// rule 21). The helper finds each kind of word it is there to catch.
#[test]
fn the_two_review_skills_name_no_ariadne_word() {
    for name in ["code-review", "architecture-review"] {
        assert_names_no_ariadne_word(name, default_skill_document(name).unwrap());
    }
    for (sample, word) in [
        ("Ariadne wakes you.", "ariadne"),
        ("Call `get_diff` first.", "get_diff"),
        ("Send it to the Orchestrator.", "orchestrator"),
        ("End the column.", "column"),
        ("Read the column report.", "column report"),
        ("Fail the step on it.", "fail the step"),
        ("Read the news.", "news"),
        ("Read your briefing.", "briefing"),
        ("Open the `Repo:` path.", "`repo:`"),
        ("Both seats read it.", "seat"),
        ("The keeper pushes.", "keeper"),
    ] {
        let found = ariadne_words_in(sample);
        assert!(found.iter().any(|w| w == word), "{sample}: {found:?}");
        let caught = std::panic::catch_unwind(|| assert_names_no_ariadne_word("sample", sample));
        assert!(caught.is_err(), "the helper passes {sample}");
    }
    // A plain word is no Ariadne word.
    assert!(ariadne_words_in("Return the work to the requester at this stage.").is_empty());
}

/// The agent seat text binds each plain word of a shipped skill to the
/// Ariadne word it stands for, names the orchestrator as the requester, and
/// names `post_to_session` as the way to reach it (006 rule 21).
#[test]
fn the_agent_seat_text_binds_each_plain_word_to_ariadne() {
    let agent = default_system_prompt(ariadne_core::Seat::Agent);
    let line = agent
        .lines()
        .find(|line| line.contains("A skill uses plain words."))
        .expect("the agent seat text binds no plain word");
    for sentence in [
        "The requester is the orchestrator.",
        "Reach it with `post_to_session`.",
        "A stage is a column.",
        "The report is the column report.",
        "To return the work is to fail the step.",
        "An update is news.",
        "Your instructions are your briefing.",
        "The primary checkout of the repository is the `Repo:` path.",
    ] {
        assert!(line.contains(sentence), "the binding line and {sentence}");
    }
    for (ariadne, plain) in WORD_LIST {
        if let Some(plain) = plain {
            let plain = plain.trim_start_matches("the ").trim_start_matches("an ");
            assert!(
                ste::sentences(line).iter().any(|s| {
                    let s = s.to_lowercase();
                    s.contains(&plain.to_lowercase()) && s.contains(&ariadne.to_lowercase())
                }),
                "the binding line ties no sentence of {plain} to {ariadne}"
            );
        }
    }
}
