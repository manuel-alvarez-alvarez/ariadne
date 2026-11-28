use super::template::assert_names_no_ariadne_word;
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
            "Run the tests, the lint and the format check of the files the diff changes, once for this verdict.",
        ),
        (
            "pr-babysit",
            "For a fix batch, run the tests, the lint and the format check of the files the batch changed.",
        ),
        (
            "pr-babysit",
            "For a base update with no fix, run no check: the hook and the forge run them.",
        ),
        (
            "coding",
            "For a code behavior change, write a failing test first",
        ),
        ("coding", "Run that test alone, never the suite around it."),
        (
            "coding",
            "For prose, inspect the diff and run the relevant existing checks.",
        ),
        (
            "coding",
            "Send the report. End it with one line per Criteria number and the proof for that criterion.",
        ),
        (
            "conflict-resolution",
            "Run the checks of what you resolved.",
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
            "On a return from review, add a new commit on top. Never amend. The merge stage squashes.",
        ),
        (
            "coding",
            "Done when the diff is structure only. One commit holds every move.",
        ),
        (
            "pr-babysit",
            "Never amend a pushed commit outside the rebase of step 6.",
        ),
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        assert!(
            document.contains(step),
            "the {name} skill does not say \"{step}\""
        );
    }
}

/// `conflict-resolution` says "pass the work, with the report" at its own
/// end, and the four other produce skills say "Send the report.": the agent
/// seat text owns the `complete_step` call and binds the report to the
/// column report (006 rule 21), and the `agent` supplement of
/// `conflict-resolution` holds its call. None of them restates a field of
/// the report, which the `complete_step` description alone defines.
#[test]
fn a_produce_skill_names_the_report_where_it_ends_a_stage() {
    for (name, ending) in [
        ("coding", "Send the report. End it with one line per"),
        ("conflict-resolution", "pass the work, with the report"),
        ("documentation", "Send the report."),
        ("research", "Send the report."),
        ("spec-writing", "Send the report."),
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        assert!(
            document.contains(ending),
            "the {name} skill does not name the report where it ends"
        );
        assert!(
            !document.contains("Fifteen lines at most"),
            "the {name} skill restates the column report's own line cap"
        );
    }
    let agent = default_system_prompt(ariadne_core::Seat::Agent);
    assert!(agent.contains("Call `complete_step` when"));
}

/// `coding`, `documentation`, `research` and `spec-writing` name no
/// Ariadne word (017 rule 16). Each Ariadne rule they held has an owner
/// outside them: the agent seat text owns the report call and the binding
/// of the requester, the stage and the report (006 rule 21). None of the
/// four has a supplement, since none holds an Ariadne rule of its own.
/// `coding` keeps the four ticket sections and the report line for each
/// Criteria number in plain words.
#[test]
fn the_four_produce_skills_name_no_ariadne_word_and_have_no_supplement() {
    for name in ["coding", "documentation", "research", "spec-writing"] {
        assert_names_no_ariadne_word(name, default_skill_document(name).unwrap());
        for seat in [
            ariadne_core::Seat::Agent,
            ariadne_core::Seat::Reviewer,
            ariadne_core::Seat::Orchestrator,
            ariadne_core::Seat::Keeper,
        ] {
            assert_eq!(
                skill_supplement(name, seat),
                None,
                "{name} has a supplement"
            );
        }
    }
    let coding = unwrapped(default_skill_document("coding").unwrap());
    for section in ["Objective", "Criteria", "Interfaces", "Out of scope"] {
        assert!(
            coding.contains(section),
            "coding drops the {section} section"
        );
    }
}

/// `coding` reads the repository rules that govern each file it changes,
/// and changes the documents those rules bind to the code in the same
/// commit: a return from review most often named a breach of a written
/// rule, a test that sleeps or a spec left behind. Its refactoring is a
/// path of its own, and a fix never rides inside it.
#[test]
fn coding_reads_the_rules_of_each_file_and_keeps_a_refactor_apart() {
    let coding = unwrapped(default_skill_document("coding").unwrap());
    for phrase in [
        "Read the repository rules that govern each file you change",
        "Read the test rules before you write a test.",
        "In the same commit, change each document that a repository rule binds to your code.",
        "### Refactoring path",
        "write characterization tests",
        "Make one kind of change at a time",
        "Report a defect you find. Fix none inside the refactor.",
    ] {
        assert!(
            coding.contains(phrase),
            "the coding skill does not say \"{phrase}\""
        );
    }
}

/// The coding paths select proof before implementation and cover the whole claim.
#[test]
fn coding_routes_changes_and_proves_each_claim() {
    let coding = unwrapped(default_skill_document("coding").unwrap());
    for phrase in [
        "Build the task, prove each change by its type, and commit once under the repository rules.",
        "Use `documentation` for a prose-only task.",
        "Classify each change as behavior, structure or prose.",
        "For structure, follow the refactoring path in place of steps 5 and 6.",
        "For prose, inspect the diff and run the relevant existing checks.",
        "Skip steps 5 and 6 for prose.",
        "Report a contradiction to the requester.",
        "Send both readings and your pick. Continue with your pick.",
        "the area spec where the repository keeps one.",
        "Examples include a spec, manual or index where the repository keeps them.",
        "Run each test file you touched whole.",
        "Take expected values from an independent source. Never recompute the answer in the assertion.",
        "Assert through public interfaces. Avoid assertions on internals that can change while behavior holds.",
        "Make assertions fail when any required part of the named claim breaks.",
    ] {
        assert!(
            coding.contains(phrase),
            "the coding skill does not say \"{phrase}\""
        );
    }
    let steps = coding
        .split_once("## Steps")
        .unwrap()
        .1
        .split("### Refactoring path")
        .next()
        .unwrap();
    assert!(
        steps.find("Classify each change").unwrap()
            < steps.find("write a failing test first").unwrap()
    );
    assert!(steps.contains("Write the simplest code that meets the criteria."));
    let tests = coding
        .split_once("### Tests")
        .unwrap()
        .1
        .split("## Never")
        .next()
        .unwrap();
    assert!(!tests.contains("Write the simplest code"));
}

/// `documentation`, `research` and `spec-writing` each send neighboring
/// work to a sibling by name. `documentation` reads the rules and software,
/// runs examples and page checks, links the page and commits once.
/// `research` cites a source for every fact and names what it did not check
/// and what would change its recommendation. It posts the full report to the
/// orchestrator. `spec-writing` sends the orchestrator its
/// open questions as one list, names the test of every criterion and marks
/// each test that does not exist yet, and commits the spec once after the
/// orchestrator agrees to it: its code does not exist yet, so a task that
/// changes code and spec together is `coding`'s, and the exclusion names it.
#[test]
fn each_writing_skill_names_its_sibling_and_its_own_proof() {
    for (name, sibling) in [
        ("documentation", "`spec-writing`"),
        ("documentation", "`coding`"),
        ("documentation", "`research`"),
        ("research", "`spec-writing`"),
        ("spec-writing", "`documentation`"),
        ("spec-writing", "`coding`"),
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        let exclusions = document
            .split_once("## When not to use")
            .unwrap()
            .1
            .split("## Steps")
            .next()
            .unwrap();
        assert!(
            exclusions.contains(sibling),
            "the {name} skill does not name {sibling} where it is not used"
        );
    }
    for (name, phrase) in [
        ("documentation", "Read the rules of the page's area"),
        ("documentation", "Read what the software does."),
        ("documentation", "Run each example yourself."),
        (
            "documentation",
            "Link the page where the reader looks for it",
        ),
        (
            "documentation",
            "Run the checks the repository names for the page, such as its link check.",
        ),
        (
            "documentation",
            "Commit once, under the repository's commit rules.",
        ),
        ("research", "Cite the source of every fact"),
        ("research", "what you did not check"),
        ("research", "what would change the recommendation"),
        (
            "research",
            "Done when the research report has all six parts.",
        ),
        (
            "research",
            "Send the full research report to the requester.",
        ),
        (
            "research",
            "Done when the requester has the full research report.",
        ),
        (
            "spec-writing",
            "Send every open question to the requester in one message.",
        ),
        ("spec-writing", "Name the test that proves it"),
        ("spec-writing", "The coding task writes that test."),
        ("spec-writing", "Mark each test that does not exist yet"),
        ("spec-writing", "every criterion names a test"),
        ("spec-writing", "the requester has agreed to the spec"),
        (
            "spec-writing",
            "Commit the spec once, under the repository's commit rules.",
        ),
        (
            "spec-writing",
            "Use when a goal needs a spec before its code exists.",
        ),
    ] {
        let document = unwrapped(default_skill_document(name).unwrap());
        assert!(
            document.contains(phrase),
            "the {name} skill does not say \"{phrase}\""
        );
    }
}
