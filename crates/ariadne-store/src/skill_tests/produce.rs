use super::*;

/// A skill scopes its checks to what its step changed.
#[test]
fn a_skill_scopes_its_own_checks_to_what_the_task_changed() {
    for (name, step) in [
        (
            "coding",
            "Run the checks of what you changed at the committed revision.",
        ),
        (
            "code-review",
            "Run each required check with no reusable result, once for this verdict.",
        ),
        (
            "pr-babysit",
            "Run the checks the batch affects, then push once.",
        ),
        (
            "coding",
            "For a code behavior change, write a failing test first",
        ),
        ("coding", "Run that test alone, never the suite around it."),
        (
            "coding",
            "For prose or a behavior-preserving refactor, choose verification",
        ),
        (
            "coding",
            "Call `complete_step` with the column report. End it with one line per Criteria number and the proof for that criterion.",
        ),
        ("debugging", "The tests of the crate you changed are green."),
        ("debugging", "Run the checks of what you changed."),
        (
            "conflict-resolution",
            "Run the tests and the lint of the files you resolved.",
        ),
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        assert!(
            document.contains(step),
            "the {name} skill does not say \"{step}\""
        );
    }
    for (name, document) in all_skills() {
        for phrase in ["whole suite", "full suite", "Start the whole test suite"] {
            assert!(
                !document.contains(phrase),
                "the {name} skill contains {phrase}"
            );
        }
    }
}

/// A task is built as one commit, and a return from review adds one on
/// top: the merge column squashes the branch onto the base.
///
/// A task is one responsibility: the orchestrator cuts it as one tracer.
/// So a branch divided into parts buys nothing and costs the review a
/// diff of half-built commits. No skill sends an agent to a slice or to
/// a small commit. The amend rule is the two committing skills' alone:
/// `coding`, whose answer to a review is a new commit, and `pr-babysit`,
/// whose pushed request an amend would rewrite under its readers.
#[test]
fn a_task_is_one_commit_and_nothing_amends_a_pushed_one() {
    for (name, document) in all_skills() {
        let document = unwrapped(document).to_lowercase();
        for divided in ["slice", "small commit"] {
            assert!(
                !document.contains(divided),
                "the {name} divides a task into a {divided}"
            );
        }
        if name == format!("{PR_BABYSIT_SKILL} skill") || name == "coding skill" {
            continue;
        }
        assert!(
            !document.contains("amend"),
            "the {name} repeats the rule about an amend"
        );
    }

    for (name, step) in [
        (
            "coding",
            "On a return from review, add a new commit on top. Never amend. The merge column squashes.",
        ),
        (
            "coding",
            "Commit the whole refactor once, after every move is green.",
        ),
        (
            "pr-babysit",
            "Never amend a pushed commit outside this rebase.",
        ),
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        assert!(
            document.contains(step),
            "the {name} skill does not say \"{step}\""
        );
    }
}

/// `coding` names the column report at its own end, and every other
/// produce skill that ends its own column names it the same way. None
/// of them restates a field of the report, which the `complete_step`
/// description alone defines.
#[test]
fn a_produce_skill_names_the_column_report_where_it_ends_a_column() {
    for name in [
        "coding",
        "debugging",
        "migration",
        "conflict-resolution",
        "documentation",
        "research",
        "spec-writing",
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        assert!(
            document.contains("Call `complete_step` with the column report"),
            "the {name} skill does not name the column report where it ends"
        );
        assert!(
            !document.contains("Fifteen lines at most"),
            "the {name} skill restates the column report's own line cap"
        );
    }
}
