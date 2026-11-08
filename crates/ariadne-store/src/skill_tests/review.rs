use super::*;

/// The review column reads the column report as unverified claims
/// before it pins the scope, and pins the scope before it decides which
/// checks to run. It can reuse only evidence for the reviewed revision
/// and required check. A must-fix finding names a trigger and ends the
/// step on `fail_step` with the return report; a finding with no
/// trigger is optional, and an optional one never fails the step.
#[test]
fn the_review_skill_reuses_matching_evidence_and_ends_on_a_step_call() {
    let text = unwrapped(default_skill_document("code-review").unwrap());
    let claims = text
        .find("Read the column report as unverified claims, not proof.")
        .expect("the code-review skill does not read the column report as unverified claims");
    let scope = text
        .find("Pin the scope to the diff.")
        .expect("the code-review skill does not pin the scope to the diff");
    let checks = text
        .find("Run each required check with no reusable result, once for this verdict.")
        .expect("the code-review skill does not run missing checks");
    assert!(
        claims < checks,
        "the code-review skill chooses checks before it reads the column report"
    );
    assert!(
        scope < checks,
        "the code-review skill chooses checks before it pins the scope"
    );
    assert_eq!(
        text.matches("once for this verdict").count(),
        1,
        "the code-review skill does not bind one check run to one verdict"
    );
    for rule in [
        "`git checkout --detach <branch>` in the task worktree",
        "Read the column report as unverified claims, not proof.",
        "its revision matches `git rev-parse HEAD`",
        "its command is a required check",
        "Run each required check with no reusable result, once for this verdict.",
        "Read code outside the diff only to judge a risk you can name.",
        "Give each finding its trigger, the input or state that breaks it, and",
        "is optional, not must-fix, and marked unverified",
        "Cap optional findings at five, the ones you have most conviction in.",
        "On a must-fix finding, call `fail_step` with the return report.",
        "Else call `complete_step` with the verdict and the optional findings.",
        "An optional finding never fails the step.",
        "A must-fix finding breaks a criterion or a written repository rule.",
        "Judge each test by reading its setup, action and assertions",
        "Never change code to see whether a test fails",
        "Edit no file",
        "`git merge-base --is-ancestor <sha> HEAD`",
        "Never rerun a check only to confirm the column report.",
    ] {
        assert!(text.contains(rule), "the code-review skill and {rule}");
    }
    assert!(
        !text.contains("Complete the step only when both axes pass."),
        "an optional finding fails no step"
    );
    assert!(
        !text.contains("`read_messages`"),
        "a second review reads the judged SHA from the conversation, not the channel"
    );
}

/// The reviewer session is fed by the daemon too (029), and the user
/// gives every approval and lands every request: the skill never
/// approves on its own, and names no merge, sleep, poll or forge CLI.
/// It names the three priorities, each with the trigger a finding must
/// name or be downgraded to P2, caps new P2 findings at five, asks for
/// changes only where a P0 or P1 stands, resolves a thread of its own
/// once a push fixed it, settles a declined or a still-open one without
/// a second warning, and suggests a fix where the edit is small and
/// local.
#[test]
fn the_pr_reviewer_skill_ranks_its_findings_and_never_approves() {
    let doc = default_skill_document(PR_REVIEWER_SKILL).expect("the pr-reviewer skill");
    let words: Vec<String> = doc
        .split(|c: char| !c.is_ascii_alphanumeric())
        .map(str::to_lowercase)
        .collect();
    for gone in ["merge", "sleep", "poll", "polling", "gh"] {
        assert!(
            !words.iter().any(|word| word == gone),
            "the pr-reviewer skill names {gone}"
        );
    }
    // Nothing in the skill names the agent's own approval.
    assert_eq!(
        words.iter().filter(|word| *word == "approve").count(),
        0,
        "the pr-reviewer skill approves on its own"
    );
    let doc = unwrapped(doc);
    for priority in ["P0: ", "P1: ", "P2: "] {
        assert!(
            doc.contains(priority),
            "the pr-reviewer skill has no {priority}"
        );
    }
    for sentence in ste::sentences(&doc) {
        if sentence.contains("`request_changes`") {
            assert!(
                sentence.contains("P0") && sentence.contains("P1"),
                "`request_changes` without P0 and P1: {sentence}"
            );
        }
    }
    for step in [
        "`get_pull_request`",
        "`get_diff`",
        "`since`",
        "`submit_review` once with only new findings",
        "P0 or P1",
        "`report_pull_request` with `reviewed_sha`",
        "without a reply",
        "Reuse evidence only when it covers the reviewed head",
        "On a later round, call `list_comments` first.",
        "For each thread you opened",
        "supervise_posts",
        "Read code outside the diff only to judge a risk you can name.",
        "Give every finding a trigger, the input or state that causes it, and a",
        "A finding with no trigger you can name is P2. Mark it unverified.",
        "Cap new P2 findings at five, the ones you have most conviction in.",
        "P1: a proven defect, unmet requirement, or repository-rule violation",
        // An unanswered thread waits; a declined fix is marked so and
        // left open; a claimed fix that is not one is said once.
        "Still there: <why>",
        "Do not reply again in that thread",
        "```suggestion block",
        "Never post a second comment for a defect",
        "call `resolve_thread` without a reply",
        "Resolve only a thread you opened",
        "one inline comment on the line of the defect",
        "## Done",
    ] {
        assert!(doc.contains(step), "the pr-reviewer skill has no {step}");
    }
    assert!(
        !doc.contains("git log"),
        "the pr-reviewer skill still reads a commit list"
    );
    assert_eq!(SkillSeat::of(PR_REVIEWER_SKILL), SkillSeat::PullRequest);
}

/// Every review skill marks a blocking or must-fix finding with a
/// trigger and caps its non-blocking findings at five. A finding with
/// no nameable trigger is downgraded and marked unverified. The four
/// task-column review skills also pin their reading to the diff, reuse
/// the column report's evidence where it covers the reviewed revision
/// (and, for `performance-review`, the same workload too), and bound
/// reading outside the diff to a risk the reviewer can name.
#[test]
fn every_review_skill_names_a_trigger_and_caps_its_optional_findings() {
    for name in ["code-review", PR_REVIEWER_SKILL, "architecture-review"] {
        let text = unwrapped(default_skill_document(name).unwrap());
        assert!(
            text.contains("trigger"),
            "the {name} skill does not name a trigger"
        );
        assert!(
            text.contains("Mark it unverified.") || text.contains("marked unverified"),
            "the {name} skill does not mark an untriggered finding unverified"
        );
        assert!(
            text.contains("at five"),
            "the {name} skill does not cap its non-blocking findings at five"
        );
    }
    for name in ["code-review", "architecture-review"] {
        let text = unwrapped(default_skill_document(name).unwrap());
        assert!(
            text.contains("Pin the scope to the diff."),
            "the {name} skill does not pin the scope to the diff"
        );
        assert!(
            text.contains("the column report"),
            "the {name} skill does not reuse the column report's evidence"
        );
        assert!(
            text.contains("only to judge a risk you can name"),
            "the {name} skill does not bound reading outside its scope to a named risk"
        );
    }
    for name in ["code-review", "architecture-review"] {
        let text = unwrapped(default_skill_document(name).unwrap());
        assert!(
            text.contains("covers this revision") || text.contains("revision and workload"),
            "the {name} skill does not bind its reused evidence to the reviewed revision"
        );
    }
}
