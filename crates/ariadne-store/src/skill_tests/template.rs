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

/// A skill is read on demand rather than on every launch, so it is capped
/// on its own rather than against the briefings' total. The caps are still
/// what a rewrite fits in: moving one is a decision, not a way round a
/// failing assertion.
///
/// This is that decision, taken once for the suite rather than a skill at
/// a time. The catalog is written to one template — a description that
/// says when to load the skill as well as what it does, a checkable bound
/// on every step that can end early, one anchor word carried through the
/// body, and the two or three excuses the agent talks itself into.
///
/// Three tiers, by how much procedure the skill carries. `orchestration`
/// and `debugging` get the most: one runs the ten phases of a whole goal,
/// the other a feedback loop the agent is talked out of at every step,
/// and both spend their length on the excuses rather than on the steps.
/// `coding` and `code-review` get the room of the skills almost every
/// task loads, and the two whose failure modes are worth spelling out.
/// Every other skill gets 2400, which is a template document with room
/// for its rules. The total is the sixteen at their tiers, so a skill
/// that grows costs a decision here rather than a quiet raid on another
/// skill's share.
///
/// `coding` gets 5400, because `testing` merged into it. `orchestration`
/// gets 4600: it staffs one agent per column on the column's rank, and
/// staffs a missing column before a retry.
///
/// The five review skills were tightened for falsifiable findings, a
/// cap on the optional ones, and the diff as scope: every blocking or
/// must-fix finding now names a trigger, a failure and the criterion,
/// behavior or written rule it breaks; a finding with none is
/// downgraded and marked unverified; the non-blocking findings are
/// capped at five; reading outside the diff is bound to a named risk;
/// and the verdict comes first with no preamble or closing summary.
/// `code-review` falls from 4100 to 3100: it drops naming the evidence
/// sources the column report now carries alone, and names the return
/// report by name instead of restating a field of it. `pr-reviewer`
/// falls from 6200 to 3750: its own summary paragraph and closing table
/// were pruned, since the trigger a finding names already answers what
/// the risk is, and its "Supervised repositories" section keeps only
/// the routing decision the pending-edit tools do not state themselves.
/// `architecture-review`, `spec-review` and `performance-review` move
/// off the shared 2400 onto their own, lower caps — 2380, 2320 and
/// 2380 — since each now reuses the column report's evidence and
/// names the trigger a blocking finding carries and the rule it
/// breaks, which cost more than the shared template budgeted, so a line
/// restating the same requirement a second time was cut to make room.
///
/// The eight produce skills were pruned to findings 1 to 7, past the
/// restated fields and the named ending, to the generic reasons and
/// slogans the review found: a description written as an instruction
/// instead of in the third person, a seat rule a skill repeated in its
/// own words, and a "Do not tell yourself" entry naming no failure.
/// Each cap comes down with the text: `coding` drops from 5400 to
/// 3300, `debugging` from 3400 to 2700, and `refactoring`, `migration`,
/// `conflict-resolution`, `documentation`, `research` and
/// `spec-writing` share 2000, down from the default 2400. The total
/// comes down with them, from 43000 to 40000.
///
/// A pass pruned every line of `orchestration`, `merge` and
/// `pr-babysit` that a tool description, the session rules, a spec or
/// another skill already said, or that cost nothing to remove.
/// `orchestration` still grew: its ticket step now names the four
/// things a ticket carries and bars pasted history, and its question
/// step words a question so a "yes" accepts the recommendation; its cap
/// moves from 4600 to 4500, the size the rewrite fits in with the usual
/// margin. `pr-babysit` lost its restated lead sentence, a log-URL tip
/// a tool's own response already carries, a redundant push-flag detail,
/// and a generic "do not tell yourself" entry that only restated the
/// no-polling session rule; its "Supervised repositories" section no
/// longer repeats what `edit_pending_comment` and
/// `delete_pending_comment` already say about when they are refused.
/// Its cap moves from 4000 to 3800, trading the one-line margin finding
/// 4 found for one that holds through a small future edit. `merge` lost
/// only its restated lead sentence, since every rule it states — the
/// refused push, the failed fast-forward, the forced push it never
/// makes — is its own case with its own trigger. It gets a cap of its
/// own, 1700, rather than the 2400 every unlisted skill shares, since a
/// landing procedure this short does not need a template's room.
///
/// The one ticket format lands in three skills at once. `orchestration`
/// writes the four sections and numbers the criteria in place of its
/// old one-line summary of what a ticket carries; its cap moves from
/// 4500 to 4650, the size the new step fits in with the usual margin.
/// `coding` reads the sections by name and ends its report one line per
/// criterion, and `code-review` gives one pass or fail line per
/// criterion and judges the diff against `## Out of scope`; both hold
/// their 3300 and 3100 caps. The total moves from 40000 to 40200.
/// The template adds routing, completion boundaries, and explicit mutation limits.
/// Coding combines its 3300 budget with refactoring's 2000, rounded to 5400.
/// Code review combines 3100, 2320, and 2380, rounded to 7900.
/// Orchestration gains 550 for eleven completion boundaries and routing.
/// Debugging gains 200 for routing and limits.
/// Architecture review gains 420 for routing, limits, and completion boundaries.
/// Migration gains 200; conflict resolution, documentation, and research gain 300 each.
/// Specification writing gains 400 for four missing completion boundaries and routing.
/// Merge gains 600 for six completion boundaries and the missing counterexample section.
/// PR babysit gains 600; PR reviewer gains 650 for eight completion boundaries each.
/// The total moves from 41200 to 45500 for the common template across thirteen documents.
#[test]
fn skill_size_caps_hold() {
    const TOTAL: usize = 45_500;
    let cap = |name: &str| match name {
        "orchestration" => 5200,
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
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("skills")
            .join(skill.name)
            .join("SKILL.md");
        assert_eq!(std::fs::read_to_string(path).unwrap(), document);
    }
    for (old, new) in MERGED_SKILLS {
        assert!(default_skill_document(old).is_none());
        assert!(default_skill_document(new).is_some());
        assert!(
            !std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("skills")
                .join(old)
                .exists()
        );
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
