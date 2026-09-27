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
        return Evaluation(0.1, "allow") if case["id"] == "ok" else Evaluation(None, "ask")

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
        self.assertEqual([(r.id, r.danger, r.label) for r in results], [("ok", 0.1, "allow"), ("no", None, "ask")])
        self.assertTrue(all(isinstance(r, EvaluationResult) and r.latency_ms >= 0 for r in results))

    def test_a_failing_case_still_tears_the_backend_down(self) -> None:
        evaluator = Recording(fail_on="no")
        with self.assertRaises(EvaluatorError):
            run.evaluate_all(evaluator, [{"id": "ok"}, {"id": "no"}, {"id": "never"}])

        self.assertEqual(evaluator.calls, ["setup", "evaluate ok", "evaluate no", "teardown"])


class ThreeWayTests(unittest.TestCase):
    def test_allow_at_or_under_the_allow_threshold(self) -> None:
        self.assertEqual(decision.three_way(0.40, 0.41, 0.90), Evaluation(0.40, "allow"))
        self.assertEqual(decision.three_way(0.41, 0.41, 0.90), Evaluation(0.41, "allow"))

    def test_deny_at_or_over_the_deny_threshold(self) -> None:
        self.assertEqual(decision.three_way(0.90, 0.41, 0.90), Evaluation(0.90, "deny"))
        self.assertEqual(decision.three_way(0.95, 0.41, 0.90), Evaluation(0.95, "deny"))

    def test_ask_between_the_thresholds_and_on_no_danger(self) -> None:
        self.assertEqual(decision.three_way(0.60, 0.41, 0.90), Evaluation(0.60, "ask"))
        self.assertEqual(decision.three_way(None, 0.41, 0.90), Evaluation(None, "ask"))


class NoulDangerTests(unittest.TestCase):
    def answer(self, noul: float) -> dict[str, Any]:
        return {"answers": {"decision": {"type": "noul", "noul": noul}}}

    def test_true_is_danger_returns_the_probability_itself(self) -> None:
        self.assertEqual(decision.noul_danger(self.answer(0.8), "true_is_danger"), 0.8)

    def test_false_is_danger_returns_the_complement(self) -> None:
        self.assertAlmostEqual(decision.noul_danger(self.answer(0.8), "false_is_danger"), 0.2)
        self.assertAlmostEqual(decision.noul_danger(self.answer(0.3), "false_is_danger"), 0.7)

    def test_an_unusable_answer_has_no_danger(self) -> None:
        self.assertIsNone(decision.noul_danger({"answers": {}}, "true_is_danger"))
        self.assertIsNone(decision.noul_danger(None, "true_is_danger"))

    def test_an_out_of_range_or_non_finite_noul_has_no_danger(self) -> None:
        self.assertIsNone(decision.noul_danger(self.answer(1.2), "true_is_danger"))
        self.assertIsNone(decision.noul_danger(self.answer(-0.1), "true_is_danger"))
        self.assertIsNone(decision.noul_danger(self.answer(float("nan")), "true_is_danger"))
        self.assertIsNone(decision.noul_danger(self.answer(float("inf")), "true_is_danger"))
        self.assertIsNone(decision.noul_danger(self.answer(True), "true_is_danger"))


class ScoreDangerTests(unittest.TestCase):
    def test_the_expected_level_divided_by_the_last_level_index(self) -> None:
        answer = {
            "answers": {
                "decision": {
                    "type": "score",
                    "score": 3.0,
                    "probabilities": {"0": 0.0, "1": 0.0, "2": 0.0, "3": 1.0},
                }
            }
        }
        self.assertEqual(decision.score_danger(answer), 1.0)

    def test_a_middle_level_is_a_fraction_of_the_last_index(self) -> None:
        answer = {"answers": {"decision": {"type": "score", "score": 1.0, "probabilities": {"0": 0.0, "1": 1.0, "2": 0.0}}}}
        self.assertEqual(decision.score_danger(answer), 0.5)

    def test_an_unusable_answer_has_no_danger(self) -> None:
        self.assertIsNone(decision.score_danger({"answers": {"decision": {"type": "score"}}}))
        self.assertIsNone(decision.score_danger(None))

    def test_an_out_of_range_or_non_finite_score_has_no_danger(self) -> None:
        probabilities = {"0": 0.5, "1": 0.5}
        self.assertIsNone(decision.score_danger({"answers": {"decision": {"score": 5.0, "probabilities": probabilities}}}))
        self.assertIsNone(decision.score_danger({"answers": {"decision": {"score": -1.0, "probabilities": probabilities}}}))
        self.assertIsNone(
            decision.score_danger({"answers": {"decision": {"score": float("nan"), "probabilities": probabilities}}})
        )

    def test_an_out_of_range_probability_in_the_distribution_has_no_danger(self) -> None:
        answer = {"answers": {"decision": {"score": 0.5, "probabilities": {"0": 1.4, "1": -0.4}}}}
        self.assertIsNone(decision.score_danger(answer))


class ChoiceDangerTests(unittest.TestCase):
    def test_the_weighted_sum_of_every_option(self) -> None:
        answer = {
            "answers": {
                "decision": {
                    "type": "choice",
                    "choice": "risky",
                    "probabilities": {"safe": 0.2, "risky": 0.8},
                }
            }
        }
        self.assertEqual(decision.choice_danger(answer, {"safe": 0.0, "risky": 1.0}), 0.8)

    def test_a_probability_with_no_weight_has_no_danger(self) -> None:
        answer = {"answers": {"decision": {"type": "choice", "choice": "x", "probabilities": {"unknown": 1.0}}}}
        self.assertIsNone(decision.choice_danger(answer, {"safe": 0.0, "risky": 1.0}))

    def test_an_unusable_answer_has_no_danger(self) -> None:
        self.assertIsNone(decision.choice_danger(None, {"safe": 0.0}))
        self.assertIsNone(decision.choice_danger({"answers": {"decision": {"type": "noul"}}}, {"safe": 0.0}))

    def test_an_out_of_range_or_non_finite_probability_has_no_danger(self) -> None:
        answer = {"answers": {"decision": {"type": "choice", "choice": "risky", "probabilities": {"risky": 1.5}}}}
        self.assertIsNone(decision.choice_danger(answer, {"risky": 1.0}))
        answer = {"answers": {"decision": {"type": "choice", "choice": "risky", "probabilities": {"risky": float("nan")}}}}
        self.assertIsNone(decision.choice_danger(answer, {"risky": 1.0}))

    def test_a_derived_danger_out_of_range_has_no_danger(self) -> None:
        answer = {"answers": {"decision": {"type": "choice", "choice": "risky", "probabilities": {"risky": 0.9}}}}
        self.assertIsNone(decision.choice_danger(answer, {"risky": 2.0}))

    def test_a_non_numeric_weight_has_no_danger(self) -> None:
        answer = {"answers": {"decision": {"type": "choice", "choice": "risky", "probabilities": {"risky": 0.9}}}}
        self.assertIsNone(decision.choice_danger(answer, {"risky": "high"}))


if __name__ == "__main__":
    unittest.main()
