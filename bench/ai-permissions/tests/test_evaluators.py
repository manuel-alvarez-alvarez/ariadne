from __future__ import annotations

import sys
import unittest
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import run
from ai_bench import decision, registry
from ai_bench.evaluator import Evaluation, EvaluationResult, Evaluator, EvaluatorError
from evaluators.kev import KevEvaluator
from evaluators.laya import LayaEvaluator


class Recording(Evaluator):
    """An evaluator that records its calls and allows the case `ok` only."""

    key = "recording"
    backend = "test"

    def __init__(self, fail_on: str | None = None) -> None:
        self.calls: list[str] = []
        self.fail_on = fail_on

    def setup(self) -> None:
        self.calls.append("setup")

    def evaluate(self, case: dict[str, Any]) -> Evaluation:
        self.calls.append("evaluate " + case["id"])
        if case["id"] == self.fail_on:
            raise EvaluatorError("broken")
        return Evaluation(0.9, "allow") if case["id"] == "ok" else Evaluation(None, "escalate")

    def teardown(self) -> None:
        self.calls.append("teardown")


class EvaluatorTests(unittest.TestCase):
    def test_every_registered_evaluator_is_a_concrete_kev_or_laya_mode(self) -> None:
        registered = registry.load_all()
        self.assertEqual(sorted(registered), ["kev_v1", "laya_v1"])
        for key, cls in registered.items():
            self.assertEqual(cls.key, key)
            self.assertTrue(cls.description, key)
            base = {"kev": KevEvaluator, "laya": LayaEvaluator}[cls.backend]
            self.assertTrue(issubclass(cls, base), key)
            cls()  # concrete: every abstract method is implemented

    def test_a_key_registered_twice_is_refused(self) -> None:
        registry.load_all()

        class Twin(KevEvaluator):
            key = "kev_v1"

            def evaluate(self, case: dict[str, Any]) -> Evaluation:
                raise AssertionError

        with self.assertRaises(EvaluatorError):
            registry.register(Twin)

    def test_an_unknown_key_names_the_list_command(self) -> None:
        with self.assertRaisesRegex(EvaluatorError, "run.py list"):
            registry.get("kev_v0")

    def test_a_run_sets_up_once_evaluates_each_case_in_order_and_tears_down(self) -> None:
        evaluator = Recording()
        results = run.evaluate_all(evaluator, [{"id": "ok"}, {"id": "no"}])

        self.assertEqual(evaluator.calls, ["setup", "evaluate ok", "evaluate no", "teardown"])
        self.assertEqual([(r.id, r.allow_score, r.label) for r in results], [("ok", 0.9, "allow"), ("no", None, "escalate")])
        self.assertTrue(all(isinstance(r, EvaluationResult) and r.latency_ms >= 0 for r in results))

    def test_a_failing_case_still_tears_the_backend_down(self) -> None:
        evaluator = Recording(fail_on="no")
        with self.assertRaises(EvaluatorError):
            run.evaluate_all(evaluator, [{"id": "ok"}, {"id": "no"}, {"id": "never"}])

        self.assertEqual(evaluator.calls, ["setup", "evaluate ok", "evaluate no", "teardown"])


class DecisionTests(unittest.TestCase):
    def answer(self, noul: float) -> dict[str, Any]:
        return {"answers": {"decision": {"type": "noul", "noul": noul}}}

    def test_a_noul_allows_only_when_its_allowing_side_clears_the_threshold(self) -> None:
        self.assertEqual(decision.noul(self.answer(0.30), 0.59, "false_is_allow"), Evaluation(0.7, "allow"))
        self.assertEqual(decision.noul(self.answer(0.45), 0.59, "false_is_allow").label, "escalate")
        self.assertEqual(decision.noul(self.answer(0.80), 0.59, "false_is_allow").label, "escalate")
        self.assertEqual(decision.noul(self.answer(0.80), 0.59, "true_is_allow"), Evaluation(0.8, "allow"))

    def test_an_unusable_answer_has_no_score_and_escalates(self) -> None:
        self.assertEqual(decision.noul({"answers": {}}, 0.1, "false_is_allow"), Evaluation(None, "escalate"))
        self.assertEqual(decision.choice(None, "allow", 0.1), Evaluation(None, "escalate"))


if __name__ == "__main__":
    unittest.main()
