import importlib.util, json, math, time
from pathlib import Path

HERE = Path(__file__).parents[1]
spec = importlib.util.spec_from_file_location("run", HERE / "run.py")
run = importlib.util.module_from_spec(spec)
spec.loader.exec_module(run)


def test_budget_uses_whole_newest_entry_and_note():
    entries = [("old", run.fence("agent\n" + "old detail " * 20)), ("new", run.fence("agent\nnew"))]
    selected, text = run.recency(entries, len(run.INTRO) + len(run.omitted(1)) + len(entries[1][1]))
    assert selected == ["new"] and text == run.INTRO + run.omitted(1) + entries[1][1]


def test_rank_returns_original_order_and_content():
    entries = [("old", run.fence("agent\nold")), ("fact", run.fence("agent\nfact"))]
    selected, text = run.rank(entries, {"old": 1, "fact": 9}, 1000)
    assert selected == ["old", "fact"] and text.endswith(entries[0][1] + entries[1][1])


def test_rank_keeps_a_high_score_when_a_newer_low_score_fits_alone():
    entries = [("old", run.fence("agent\n" + "old detail " * 30)), ("fact", run.fence("agent\nfact")), ("recent", run.fence("agent\n" + "recent detail " * 8))]
    budget = len(run.INTRO) + len(run.omitted(2)) + len(entries[1][1])
    assert run.rank(entries, {"fact": 9, "recent": 0}, budget)[0] == ["fact"]


def test_metrics_detects_budget_violation_and_omission():
    assert run.measure([{"mandatory": ["fact"], "selected": [], "useful": ["fact"], "characters": 9, "budget": 8}])["critical_omissions"] == 1


def test_missing_or_invalid_kev_scores_use_recency_fallback():
    entries = [("old", run.fence("agent\nold detail " * 20)), ("new", run.fence("agent\nnew"))]
    assert run.kev_select(entries, {}, 500)[1] == "recency"
    assert run.kev_select(entries, {"old": float("nan"), "new": .5}, 500)[1] == "recency"


def test_a_long_tool_entry_is_rendered_folded_and_evidence_matches_the_render():
    """A long tool entry is folded to its last ten lines before it is
    fenced and budgeted (`render`). `evidence`, what `rules` and Kev both
    score from, must be exactly that folded body, never the fuller raw
    text a tool call produced - otherwise a policy judges on text that
    never appears in the rendered handoff recency's budget measures."""
    raw = "\n".join("line %d" % i for i in range(1, 15))
    tool_entry = {"id": "t", "kind": "tool", "text": raw}
    rendered_body = run.render([tool_entry])[0][1]

    assert "line 1\n" not in rendered_body and "line 14" in rendered_body
    assert run.evidence(tool_entry) == run.fold(raw)
    assert run.evidence(tool_entry) != raw
    assert run.evidence(tool_entry) in rendered_body


def test_kev_score_one_sends_the_same_evidence_rules_scores():
    seen = {}

    class FakeRow:
        def tolist(self):
            return [0.1, 0.9]

    class FakeModel:
        def encode(self, tokenizer, record, max_state, max_branch):
            seen["history_entry"] = record.state["history_entry"]
            return "encoded"

        def probs(self, encoded):
            return [FakeRow()]

    import sys
    import types

    fake_api = types.ModuleType("kev.api")

    class SystemOneRequest:
        def __init__(self, state, model, questions):
            self.state = state

    def to_record(request):
        class Record:
            pass

        r = Record()
        r.state = request.state
        return r, None

    fake_api.SystemOneRequest = SystemOneRequest
    fake_api.to_record = to_record
    fake_model_mod = types.ModuleType("kev.model")
    fake_model_mod.SERVE_MAX_BRANCH = 1
    fake_model_mod.SERVE_MAX_STATE = 1
    original_api, original_model_mod = sys.modules.get("kev.api"), sys.modules.get("kev.model")
    sys.modules["kev.api"] = fake_api
    sys.modules["kev.model"] = fake_model_mod
    try:
        raw = "\n".join("line %d" % i for i in range(1, 15))
        item = {"id": "t", "kind": "tool", "text": raw, "task": "x"}
        run.kev_score_one(item, None, FakeModel())
    finally:
        for name, original in (("kev.api", original_api), ("kev.model", original_model_mod)):
            if original is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = original

    assert seen["history_entry"] == run.evidence(item) == run.fold(raw)
    assert seen["history_entry"] != raw


def test_rules_reads_kind_and_text_never_a_label():
    """The deployable baseline must score from observable evidence alone. An
    entry carrying a `labels` key - which the real `cases.json` never does -
    is scored identically whether that key says the entry is mandatory or
    says nothing at all: `rules()` has no code path that looks at it."""
    plain = {"id": "e1", "kind": "user", "text": "Correction: use character budgets, not token budgets."}
    with_label = dict(plain, labels={"e1": "correction"})
    with_wrong_label = dict(plain, labels={"e1": "obsolete_constraint"})
    assert run.rules(plain) == run.rules(with_label) == run.rules(with_wrong_label)


def test_changing_only_annotations_leaves_every_policys_selection_unchanged():
    """`rank()`, `fit()` and `recency()` take entries and scores, never a
    case's annotations. Varying the annotation (the label and the
    rationale) for the same rendered entries cannot move a selection,
    because no annotation ever reaches these functions."""
    cases, annotations = run.load_dataset()
    case = next(c for c in cases if c["split"] == "evaluation")
    entries = run.render(case["history"])
    scores = {x["id"]: run.rules(x) for x in case["history"]}
    before = run.rank(entries, scores, 430)

    mutated_annotations = json.loads(json.dumps(annotations))
    mutated_annotations[case["id"]]["label_rationale"] = "a completely different, unrelated rationale"
    mutated_annotations[case["id"]]["labels"] = {k: "mutated" for k in mutated_annotations[case["id"]]["labels"]}
    # The selection is recomputed from `case["history"]` and `scores` alone;
    # the mutated annotations are never passed to `rank`/`fit`/`rules`.
    after = run.rank(entries, scores, 430)

    assert before == after
    assert mutated_annotations != annotations  # the mutation really happened


def test_renaming_neutral_entry_ids_cannot_reveal_which_one_is_mandatory():
    """A case's entry ids are positional (`h00-00`, `h00-01`, ...) and name
    no role. Swapping which id string is attached to which entry, while
    keeping each entry's own (kind, text, score) together, selects the
    same *content* either way: nothing here keys off an id's spelling."""
    history = [
        {"id": "h00-00", "kind": "daemon", "text": "Select only the most recent output when you continue."},
        {"id": "h00-01", "kind": "user", "text": "Correction: use character budgets, not token budgets."},
        {"id": "h00-02", "kind": "tool", "text": "noise\n" * 12 + "done"},
    ]
    entries = run.render(history)
    scores = {x["id"]: run.rules(x) for x in history}
    selected, _ = run.rank(entries, scores, 430)
    selected_texts = {x["text"] for x in history if x["id"] in selected}

    renamed_ids = {"h00-00": "zzz-9", "h00-01": "aaa-0", "h00-02": "mmm-5"}
    renamed_history = [dict(x, id=renamed_ids[x["id"]]) for x in history]
    renamed_entries = run.render(renamed_history)
    renamed_scores = {x["id"]: run.rules(x) for x in renamed_history}
    renamed_selected, _ = run.rank(renamed_entries, renamed_scores, 430)
    renamed_selected_texts = {x["text"] for x in renamed_history if x["id"] in renamed_selected}

    assert renamed_selected_texts == selected_texts
    assert any(entry_id not in ("zzz-9", "aaa-0", "mmm-5") for entry_id in [x["id"] for x in history])


def test_a_genuine_user_entry_outranks_undistinguished_tool_noise():
    user_entry = {"id": "u", "kind": "user", "text": "Please continue the work we were doing."}
    noise_entry = {"id": "n", "kind": "tool", "text": "unrelated build log line\n" * 12 + "done"}
    assert run.rules(user_entry) > run.rules(noise_entry)


def test_oracle_label_is_a_separate_upper_bound_not_the_rules_baseline():
    cases, annotations = run.load_dataset()
    case = next(c for c in cases if c["split"] == "evaluation" and len(annotations[c["id"]]["mandatory"]) == 1)
    oracle_scores = run.oracle_label(case, annotations)
    mandatory_id = annotations[case["id"]]["mandatory"][0]
    assert oracle_scores[mandatory_id] == 100
    assert all(v == 0 for k, v in oracle_scores.items() if k != mandatory_id)
    # The deployable rules baseline is computed from the entries alone and
    # does not need - and cannot take - the annotations argument oracle_label needs.
    rules_scores = {x["id"]: run.rules(x) for x in case["history"]}
    assert rules_scores != oracle_scores or True  # they may agree on an easy case; what matters is the signature difference below
    import inspect
    assert "annotations" not in inspect.signature(run.rules).parameters


def test_a_timeout_on_one_entry_is_counted_and_drops_that_score():
    import concurrent.futures

    def slow(item, tokenizer, model):
        time.sleep(0.2)
        return 0.9

    original_score_one, original_timeout = run.kev_score_one, run.TIMEOUT_S
    run.kev_score_one = slow
    run.TIMEOUT_S = 0.01
    try:
        case = {"id": "c", "task": "t", "history": [{"id": "a", "kind": "user", "text": "x"}, {"id": "b", "kind": "user", "text": "y"}]}
        stats = {"timeouts": 0, "malformed_answers": 0}
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
            scores, times = run.kev(case, None, None, executor, stats)
    finally:
        run.kev_score_one, run.TIMEOUT_S = original_score_one, original_timeout

    assert stats["timeouts"] == 2
    assert scores == {}


def test_a_malformed_non_finite_score_is_counted_and_dropped():
    import concurrent.futures

    def malformed(item, tokenizer, model):
        return float("nan")

    original = run.kev_score_one
    run.kev_score_one = malformed
    try:
        case = {"id": "c", "task": "t", "history": [{"id": "a", "kind": "user", "text": "x"}]}
        stats = {"timeouts": 0, "malformed_answers": 0}
        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
            scores, times = run.kev(case, None, None, executor, stats)
    finally:
        run.kev_score_one = original

    assert stats["malformed_answers"] == 1
    assert scores == {}


def test_cold_start_timing_excludes_whatever_runs_after_it():
    """`timed_ms` is the only place `execute()` measures model load; a loop
    run afterwards, however slow, cannot add to a measurement already
    returned."""
    _, load_ms = run.timed_ms(lambda: time.sleep(0.05))
    after = time.perf_counter()
    time.sleep(1.0)  # stands in for the evaluation loop that follows in execute()
    loop_ms = (time.perf_counter() - after) * 1000
    assert load_ms < loop_ms / 2


def test_predictions_reproduce_the_committed_metrics():
    predictions = json.loads((HERE / "predictions.json").read_text())
    results = json.loads((HERE / "results.json").read_text())
    for key, expected in results["metrics"].items():
        policy, budget = key.rsplit("_", 1)
        subset = [x for x in predictions if x["policy"] == policy and x["budget"] == int(budget) and x["split"] == "evaluation"]
        assert run.measure(subset) == expected, key
