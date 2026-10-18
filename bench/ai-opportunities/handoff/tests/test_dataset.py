import importlib.util, json
from pathlib import Path

HERE = Path(__file__).parents[1]
spec = importlib.util.spec_from_file_location("dataset", HERE / "dataset.py")
dataset = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dataset)


def test_committed_dataset_has_at_least_60_cases_and_20_reserved_for_evaluation():
    cases = json.loads((HERE / "cases.json").read_text())
    assert len(cases) >= 60
    assert sum(c["split"] == "evaluation" for c in cases) >= 20


def test_committed_dataset_covers_every_template():
    cases = json.loads((HERE / "cases.json").read_text())
    assert {t for t in dataset.TEMPLATES} <= {c["family"].split("-")[0] for c in cases}


def test_the_committed_dataset_has_no_split_violation():
    cases = json.loads((HERE / "cases.json").read_text())
    assert dataset.split_violations(cases) == []


def test_split_violations_detects_a_family_split_across_dev_and_eval():
    cases = [
        {"id": "a", "split": "development", "family": "f", "history": [{"kind": "user", "text": "x"}]},
        {"id": "b", "split": "evaluation", "family": "f", "history": [{"kind": "user", "text": "y"}]},
    ]
    kinds = {v[0] for v in dataset.split_violations(cases)}
    assert "family_split_mismatch" in kinds


def test_split_violations_detects_a_semantic_template_shared_across_splits():
    """A template id spanning both splits is a leak even where the family
    string and the rendered text both differ: evaluation would still be
    measuring a scenario development already saw."""
    cases = [
        {"id": "a", "split": "development", "family": "fam-a", "template": "correction", "history": [{"kind": "user", "text": "Correction: one phrasing."}]},
        {"id": "b", "split": "evaluation", "family": "fam-b", "template": "correction", "history": [{"kind": "user", "text": "Correction: a completely different phrasing."}]},
    ]
    kinds = {v[0] for v in dataset.split_violations(cases)}
    assert "template_split_mismatch" in kinds


def test_split_violations_accepts_the_same_template_name_confined_to_one_split():
    cases = [
        {"id": "a", "split": "development", "family": "fam-a", "template": "correction", "history": [{"kind": "user", "text": "Correction: one phrasing."}]},
        {"id": "b", "split": "development", "family": "fam-b", "template": "correction", "history": [{"kind": "user", "text": "Correction: a different phrasing."}]},
    ]
    assert dataset.split_violations(cases) == []


def test_every_template_in_the_committed_dataset_sits_wholly_in_one_split():
    cases = json.loads((HERE / "cases.json").read_text())
    template_splits: dict[str, set[str]] = {}
    for case in cases:
        template_splits.setdefault(case["template"], set()).add(case["split"])
    straddling = {t: splits for t, splits in template_splits.items() if len(splits) > 1}
    assert straddling == {}
    assert set(dataset.DEV_TEMPLATES).isdisjoint(dataset.EVAL_TEMPLATES)


def test_the_reserved_evaluation_split_covers_the_required_scenario_kinds():
    """Criterion: multiple relevant entries, an obsolete constraint
    superseded by a correction, corrections, a quoted directive, and
    distracting recent output must all be measurable from evaluation
    alone, since development cases are never scored."""
    required = {"correction", "obsolete_then_correction", "multi_mandatory", "quoted_directive_trap", "distracting_recent_output"}
    assert required <= set(dataset.EVAL_TEMPLATES)


def test_split_violations_detects_a_duplicate_history_across_splits():
    history = [{"kind": "user", "text": "same content either way"}]
    cases = [
        {"id": "a", "split": "development", "family": "fam-a", "history": history},
        {"id": "b", "split": "evaluation", "family": "fam-b", "history": history},
    ]
    kinds = {v[0] for v in dataset.split_violations(cases)}
    assert "duplicate_content_across_splits" in kinds


def test_split_violations_accepts_a_paraphrase_family_kept_in_one_split():
    cases = [
        {"id": "a", "split": "development", "family": "fam", "history": [{"kind": "user", "text": "x"}]},
        {"id": "b", "split": "development", "family": "fam", "history": [{"kind": "user", "text": "a paraphrase of x"}]},
    ]
    assert dataset.split_violations(cases) == []


def test_no_entry_or_case_in_cases_json_carries_a_label_or_mandatory_marker():
    """Policy input must never leak an annotation. No key on a case or a
    history entry names a label, a mandatory marker, or a rationale."""
    cases = json.loads((HERE / "cases.json").read_text())
    forbidden = {"label", "labels", "mandatory", "label_rationale"}
    for case in cases:
        assert forbidden.isdisjoint(case.keys()), case["id"]
        for entry in case["history"]:
            assert forbidden.isdisjoint(entry.keys()), entry["id"]


def test_entry_ids_are_positional_and_name_no_label():
    cases = json.loads((HERE / "cases.json").read_text())
    labels = {"constraint", "correction", "blocker", "decision", "changed_file", "tool_result", "obsolete_constraint", "user_task"}
    for case in cases:
        for entry in case["history"]:
            assert not any(label in entry["id"] for label in labels), entry["id"]


def test_annotations_mandatory_ids_exist_in_their_cases_history():
    cases = {c["id"]: {e["id"] for e in c["history"]} for c in json.loads((HERE / "cases.json").read_text())}
    annotations = json.loads((HERE / "annotations.json").read_text())
    for case_id, annotation in annotations.items():
        for entry_id in annotation["mandatory"]:
            assert entry_id in cases[case_id], (case_id, entry_id)


def test_every_history_in_the_committed_dataset_is_rendered_distinct():
    cases = json.loads((HERE / "cases.json").read_text())
    rendered = [tuple((e["kind"], e["text"]) for e in c["history"]) for c in cases]
    assert len(set(rendered)) == len(rendered)


def test_build_is_deterministic():
    cases_a, annotations_a = dataset.build()
    cases_b, annotations_b = dataset.build()
    assert cases_a == cases_b
    assert annotations_a == annotations_b
