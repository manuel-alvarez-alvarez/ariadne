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
            !summary.is_empty() && summary.len() <= 140,
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

/// Each of the thirteen skills has a cap for its procedure and template sections.
/// A separate total cap prevents growth from consuming another skill's unused budget.
/// The description cap belongs to the catalog test above.
///
/// Four merges preserve their procedures in the absorbing documents.
/// Testing previously merged into coding, whose later pruning left a 3300 budget.
/// `refactoring` now merges into `coding`, adding its 2000 budget and rounding to 5400.
/// `spec-review` and `performance-review` now merge into `code-review`.
/// Their 2320 and 2380 budgets join code review's 3100, rounded to 7900.
/// The three remaining review skills are code-review, architecture-review, and pr-reviewer.
///
/// The template adds sibling routing, completion boundaries, and explicit mutation limits.
/// Orchestration moves from 4650 to 5200 for eleven completion boundaries and routing.
/// It moves to 5250 to name the `wait_for_user` field of each part of a question.
/// Debugging moves from 2700 to 2900 for routing and limits.
/// Architecture review moves from 2380 to 2800 for routing, limits, and completion boundaries.
/// Migration moves from 2000 to 2200 for routing and two completion boundaries.
/// Conflict resolution, documentation, and research each move from 2000 to 2300.
/// Their routing, limits, and missing completion boundaries use the extra space.
/// Specification writing moves from 2000 to 2400 for routing and four missing completion boundaries.
/// Merge moves from 2300 to 2900 for six completion boundaries and the missing counterexample section.
/// PR babysit moves from 4300 to 4900; PR reviewer moves from 3750 to 4400.
/// Each gains eight completion boundaries plus routing and explicit limits.
///
/// The total moves from 41200 to 45500 for thirteen documents with the common template.
/// The three merges remove catalog entries but preserve the procedures and their costs.
/// Routing and completion boundaries consume the additional 4300 characters across the catalog.
/// The total leaves a small margin beyond the current documents, below the combined individual caps.
#[test]
fn skill_size_caps_hold() {
    const TOTAL: usize = 45_500;
    let cap = |name: &str| match name {
        "orchestration" => 5250,
        "debugging" => 2900,
        "code-review" => 7900,
        "coding" => 5400,
        "architecture-review" => 2800,
        "migration" => 2200,
        "conflict-resolution" => 2300,
        "documentation" => 2300,
        "research" => 2300,
        "spec-writing" => 2400,
        "merge" => 2900,
        "pr-babysit" => 4900,
        "pr-reviewer" => 4400,
        _ => unreachable!(),
    };

    let read = |skill: &BuiltinSkill| skill_text(skill.document).len();
    for skill in &BUILTIN_SKILLS {
        println!("{:5}  {}", read(skill), skill.name);
    }
    for skill in &BUILTIN_SKILLS {
        assert!(
            read(skill) <= cap(skill.name),
            "the {} skill is {} characters, over its {}",
            skill.name,
            read(skill),
            cap(skill.name)
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
            "narrate progress",
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
    assert_eq!(BUILTIN_SKILLS.len(), 13);
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
        let description = skill_summary(document).unwrap();
        assert!(
            description.starts_with("Use when "),
            "{}: description trigger",
            skill.name
        );
        assert_eq!(
            description.matches('.').count(),
            1,
            "{}: triggers alone",
            skill.name
        );
        assert!(
            !description.contains(" you "),
            "{}: third person",
            skill.name
        );
        let headings: Vec<_> = body
            .lines()
            .filter(|line| line.starts_with("## "))
            .collect();
        let required = [
            "## When not to use",
            "## Steps",
            "## Never",
            "## Do not tell yourself",
            "## Done",
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
        let mut lines = body.lines().filter(|line| !line.is_empty());
        assert!(lines.next().unwrap().starts_with("# "));
        assert!(
            lines.next().unwrap().contains("Done when"),
            "{}: outcome",
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
    ] {
        let places = all_defaults()
            .into_iter()
            .chain(all_skills())
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
