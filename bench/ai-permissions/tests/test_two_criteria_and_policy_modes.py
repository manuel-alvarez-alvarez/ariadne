from __future__ import annotations

import contextlib
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import run
from ai_bench import decision
from ai_bench.evaluator import Evaluation, EvaluationResult
from evaluators.kev import kev_v18, kev_v24, kev_v25, kev_v26, kev_v27, kev_v28


class TwoCriteriaRequestTests(unittest.TestCase):
    """`kev_v26` keeps `kev_v25`'s allow and deny criteria word for word, and drops `ask`."""

    def test_the_criteria_are_the_allow_and_deny_of_the_winner_word_for_word(self) -> None:
        question = kev_v26.QUESTIONS["decision"]
        self.assertEqual(question["type"], "score")
        self.assertEqual(question["criteria"], [kev_v18.LEVELS[0], kev_v18.LEVELS[2]])
        self.assertEqual(question["instructions"], kev_v25.QUESTIONS["decision"]["instructions"])

    def test_the_state_facts_rules_caps_and_run_are_kev_v25s_and_the_temperature_is_2_5(self) -> None:
        self.assertIs(kev_v26.state, kev_v24.state)
        self.assertEqual(kev_v26.FACTS, kev_v25.FACTS)
        self.assertEqual(kev_v26.RULES, kev_v25.RULES)
        self.assertEqual(kev_v26.CAPS, kev_v25.CAPS)
        self.assertEqual(kev_v26.RUN, kev_v25.RUN)
        self.assertEqual(kev_v26.TEMPERATURE, 2.5)

    def test_the_danger_of_a_two_level_answer_is_p_of_deny(self) -> None:
        answer = {"answers": {"decision": {"type": "score", "score": 0.3, "probabilities": {"0": 0.7, "1": 0.3}}}}

        self.assertAlmostEqual(kev_v26.danger(answer), 0.3)


class ProbabilityPolicyRequestTests(unittest.TestCase):
    """`kev_v27` sends `kev_v25`'s request word for word; only the label policy differs."""

    def test_the_request_is_the_winners_word_for_word(self) -> None:
        self.assertIs(kev_v27.QUESTIONS, kev_v25.QUESTIONS)
        self.assertIs(kev_v27.state, kev_v25.state)
        self.assertEqual(kev_v27.FACTS, kev_v25.FACTS)
        self.assertEqual(kev_v27.RULES, kev_v25.RULES)
        self.assertEqual(kev_v27.CAPS, kev_v25.CAPS)


class OutsideReadRequestTests(unittest.TestCase):
    """`kev_v28` sends `kev_v25`'s request with one more item in the `allow` criterion: the
    read outside the workspace of the label rule of 2026-09-30."""

    def test_the_allow_criterion_gains_the_read_outside_the_workspace_and_nothing_else(self) -> None:
        question = kev_v28.QUESTIONS["decision"]
        allow, ask, deny = question["criteria"]
        item = "reading, listing and searching files outside the workspace, with no credential and no transfer"

        self.assertEqual(question["type"], "score")
        self.assertEqual(question["instructions"], kev_v25.QUESTIONS["decision"]["instructions"])
        self.assertEqual(allow.replace("; " + item, "", 1), kev_v18.LEVELS[0])
        self.assertIn("reading, searching and listing files; " + item + "; editing", allow)
        self.assertEqual([ask, deny], kev_v18.LEVELS[1:])

    def test_the_state_facts_rules_caps_and_run_are_kev_v25s_and_the_temperature_is_0_6(self) -> None:
        self.assertIs(kev_v28.state, kev_v24.state)
        self.assertIs(kev_v28.danger, kev_v25.danger)
        self.assertEqual(kev_v28.FACTS, kev_v25.FACTS)
        self.assertEqual(kev_v28.RULES, kev_v25.RULES)
        self.assertEqual(kev_v28.CAPS, kev_v25.CAPS)
        self.assertEqual(kev_v28.RUN, kev_v25.RUN)
        self.assertEqual(kev_v28.TEMPERATURE, 0.6)


def score_answer(p_allow: float, p_deny: float) -> dict[str, object]:
    p_ask = 1.0 - p_allow - p_deny
    probabilities = {"0": p_allow, "1": p_ask, "2": p_deny}
    score = sum(int(level) * probability for level, probability in probabilities.items())
    return {"answers": {"decision": {"type": "score", "score": score, "probabilities": probabilities}}}


class Derived:
    def __init__(self, risk_tags: list[str] | None = None) -> None:
        self.risk_tags = risk_tags or []


class ProbabilityPolicyDecisionTests(unittest.TestCase):
    """`kev_v27.decision`: `allow` when only P(level 0) is at its bound, `deny` when only
    P(the last level) is, `ask` otherwise, and `ask` when both are."""

    def test_allow_when_only_p_allow_holds(self) -> None:
        found = kev_v27.decision(score_answer(kev_v27.ALLOW_PROBABILITY, 0.0), Derived())

        self.assertEqual(found.label, "allow")
        self.assertAlmostEqual(found.p_allow, kev_v27.ALLOW_PROBABILITY)

    def test_deny_when_only_p_deny_holds(self) -> None:
        found = kev_v27.decision(score_answer(0.0, kev_v27.DENY_PROBABILITY), Derived())

        self.assertEqual(found.label, "deny")
        self.assertAlmostEqual(found.p_deny, kev_v27.DENY_PROBABILITY)

    def test_ask_when_neither_holds(self) -> None:
        found = kev_v27.decision(score_answer(0.0, 0.0), Derived())

        self.assertEqual(found.label, "ask")

    def test_ask_when_both_hold(self) -> None:
        with mock.patch.object(kev_v27, "ALLOW_PROBABILITY", 0.4), mock.patch.object(
            kev_v27, "DENY_PROBABILITY", 0.4
        ):
            found = kev_v27.decision(score_answer(0.5, 0.5), Derived())

        self.assertEqual(found.label, "ask")
        self.assertAlmostEqual(found.p_allow, 0.5)
        self.assertAlmostEqual(found.p_deny, 0.5)

    def test_a_cap_refuses_the_allow(self) -> None:
        found = kev_v27.decision(score_answer(kev_v27.ALLOW_PROBABILITY, 0.0), Derived(risk_tags=["privileged"]))

        self.assertEqual((found.label, found.cap), ("ask", "privileged"))

    def test_no_usable_score_is_ask(self) -> None:
        found = kev_v27.decision({"answers": {}}, Derived())

        self.assertEqual(found.label, "ask")


class ScoreBoundsTests(unittest.TestCase):
    def test_score_bounds_are_the_first_and_last_level_probabilities(self) -> None:
        answer = {
            "answers": {"decision": {"type": "score", "score": 1.0, "probabilities": {"0": 0.2, "1": 0.3, "2": 0.5}}}
        }

        self.assertEqual(decision.score_bounds(answer), (0.2, 0.5))

    def test_score_bounds_of_an_unusable_answer_is_none(self) -> None:
        self.assertIsNone(decision.score_bounds({"answers": {}}))


def probe_record(case_id: str, case_set: str, probabilities: list[float]) -> dict[str, object]:
    return {
        "id": case_id,
        "set": case_set,
        "expected": "allow" if case_set in ("safe", "real") else "deny",
        "operation": None,
        "risk_tags": [],
        "pair": None,
        "derived": {"operation_hint": None, "risk_tags": [], "rule": None},
        "latency_ms": 0.0,
        "probabilities": {"kev_v27": {"decision": probabilities}},
    }


class MeasurePolicyOnAScoreQuestionTests(unittest.TestCase):
    """`run.py measure policy` reads the probe records of a `score` mode, not only a `choice`
    one: it takes the first and the last of the `decision` probabilities and derives them at
    the given temperature, as it does for a `choice`."""

    def test_measure_policy_prints_a_pair_for_a_score_mode(self) -> None:
        records = [probe_record("safe-1", "safe", [0.90, 0.08, 0.02]), probe_record("risky-1", "adversarial", [0.05, 0.15, 0.80])]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "probe.jsonl"
            path.write_text("\n".join(json.dumps(record) for record in records) + "\n", encoding="utf-8")
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["measure", "policy", str(path), "--evaluator", "kev_v27", "--temperature", "1.0"])

        self.assertEqual(code, 0)
        text = output.getvalue()
        self.assertIn("allow probability", text)
        self.assertIn("0.1000", text)  # risky-1's P(allow) 0.05, plus the default margin 0.05
        self.assertIn("0.0700", text)  # safe-1's P(deny) 0.02, plus the default margin 0.05

    def test_measure_policy_asks_when_both_bounds_hold_at_temperature_one(self) -> None:
        records = [probe_record("elevated-1", "elevated", [0.05, 0.95, 0.0]), probe_record("safe-1", "safe", [0.9, 0.0, 0.1])]
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "probe.jsonl"
            path.write_text("\n".join(json.dumps(record) for record in records) + "\n", encoding="utf-8")
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(
                    ["measure", "policy", str(path), "--evaluator", "kev_v27", "--temperature", "1.0", "--margin", "0.0"]
                )

        self.assertEqual(code, 0)
        text = output.getvalue()
        self.assertIn("0.0500", text)  # allow probability: elevated-1's own P(allow), margin 0
        self.assertIn("0.1000", text)  # deny probability: safe-1's own P(deny), margin 0
        # safe-1 holds both bounds and is `ask`, so it is not among the safe cases denied.
        lines = text.splitlines()
        self.assertEqual(lines[-1].split()[-1], "0")


class ProbabilitySelectionTests(unittest.TestCase):
    """`run.py select` on a probability-policy CSV: it prints the probability pair, not a
    danger threshold pair, and the table at that pair."""

    def test_select_prints_the_probability_pair(self) -> None:
        cases = [
            {"id": "safe-1", "set": "safe", "expected": "allow"},
            {"id": "safe-2", "set": "safe", "expected": "allow"},
            {"id": "risky-1", "set": "elevated", "expected": "ask"},
            {"id": "risky-2", "set": "adversarial", "expected": "deny"},
        ]
        results = [
            EvaluationResult("safe-1", 0.05, "allow", 1.0, p_allow=0.90, p_deny=0.02),
            EvaluationResult("safe-2", 0.20, "ask", 1.0, p_allow=0.50, p_deny=0.10),
            EvaluationResult("risky-1", 0.40, "ask", 1.0, p_allow=0.30, p_deny=0.20),
            EvaluationResult("risky-2", 0.90, "deny", 1.0, p_allow=0.05, p_deny=0.80),
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v27", cases, results)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["select", directory, "--margin", "0.05"])

        self.assertEqual(code, 0)
        text = output.getvalue()
        self.assertIn("pair: allow_probability 0.3500 / deny_probability 0.1500", text)
        self.assertIn("table at that pair", text)

    def test_select_of_a_csv_with_no_probability_columns_uses_the_danger_pair(self) -> None:
        cases = [
            {"id": "safe-1", "set": "safe", "expected": "allow"},
            {"id": "risky-1", "set": "adversarial", "expected": "deny"},
        ]
        results = [EvaluationResult("safe-1", 0.05, "allow", 1.0), EvaluationResult("risky-1", 0.10, "ask", 1.0)]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v25", cases, results)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["select", directory, "--margin", "0.05"])

        self.assertEqual(code, 0)
        self.assertIn("pair: allow_threshold 0.0500 / deny_threshold 0.1000", output.getvalue())


if __name__ == "__main__":
    unittest.main()
