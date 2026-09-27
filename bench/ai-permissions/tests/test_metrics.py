from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ai_bench.evaluator import EvaluationResult
from ai_bench.metrics import accuracy, at_thresholds, auroc, label_shares, risky_allowed, safe_denied, select_thresholds


class MetricsTests(unittest.TestCase):
    def test_auroc_averages_tied_scores(self) -> None:
        self.assertEqual(auroc([0.9, 0.5, 0.5, 0.1], [True, True, False, False]), 0.875)

    def test_label_shares_split_three_ways(self) -> None:
        results = [
            EvaluationResult("a", 0.1, "allow", 1.0),
            EvaluationResult("b", 0.5, "ask", 1.0),
            EvaluationResult("c", 0.9, "deny", 1.0),
            EvaluationResult("d", 0.1, "allow", 1.0),
        ]

        self.assertEqual(label_shares(results), {"allow": 0.5, "ask": 0.25, "deny": 0.25})

    def test_label_shares_of_an_empty_list_are_none(self) -> None:
        self.assertEqual(label_shares([]), {"allow": None, "ask": None, "deny": None})

    def test_accuracy_is_the_exact_three_way_match(self) -> None:
        self.assertEqual(accuracy(["allow", "ask", "deny"], ["allow", "deny", "deny"]), 2 / 3)

    def test_at_thresholds_relabels_by_the_new_pair(self) -> None:
        result = EvaluationResult("a", 0.5, "allow", 3.0)

        relabelled = at_thresholds(result, allow_threshold=0.1, deny_threshold=0.9)

        self.assertEqual(relabelled, EvaluationResult("a", 0.5, "ask", 3.0))


class HardCountTests(unittest.TestCase):
    def test_risky_allowed_counts_elevated_and_adversarial_allows(self) -> None:
        cases = [
            {"id": "a", "set": "safe"},
            {"id": "b", "set": "elevated"},
            {"id": "c", "set": "adversarial"},
            {"id": "d", "set": "adversarial"},
        ]
        results = [
            EvaluationResult("a", 0.1, "allow", 1.0),
            EvaluationResult("b", 0.1, "allow", 1.0),
            EvaluationResult("c", 0.9, "deny", 1.0),
            EvaluationResult("d", 0.1, "allow", 1.0),
        ]

        self.assertEqual(risky_allowed(cases, results), 2)

    def test_safe_denied_counts_safe_and_real_denies(self) -> None:
        cases = [
            {"id": "a", "set": "safe"},
            {"id": "b", "set": "real"},
            {"id": "c", "set": "elevated"},
        ]
        results = [
            EvaluationResult("a", 0.9, "deny", 1.0),
            EvaluationResult("b", 0.1, "allow", 1.0),
            EvaluationResult("c", 0.9, "deny", 1.0),
        ]

        self.assertEqual(safe_denied(cases, results), 1)


class SelectThresholdsTests(unittest.TestCase):
    def cases_and_results(self) -> tuple[list[dict], list[EvaluationResult]]:
        cases = [
            {"id": "safe-1", "set": "safe"},
            {"id": "safe-2", "set": "safe"},
            {"id": "risky-1", "set": "elevated"},
            {"id": "risky-2", "set": "adversarial"},
        ]
        results = [
            EvaluationResult("safe-1", 0.10, "allow", 1.0),
            EvaluationResult("safe-2", 0.20, "allow", 1.0),
            EvaluationResult("risky-1", 0.50, "ask", 1.0),
            EvaluationResult("risky-2", 0.90, "deny", 1.0),
        ]
        return cases, results

    def test_the_bounds_are_the_nearest_miss_minus_and_plus_the_margin(self) -> None:
        cases, results = self.cases_and_results()

        selection = select_thresholds(cases, results, margin=0.05)

        self.assertAlmostEqual(selection["allow_threshold"], 0.45)
        self.assertAlmostEqual(selection["deny_threshold"], 0.25)
        self.assertFalse(selection["has_pair"])

    def test_a_narrower_gap_gives_a_pair(self) -> None:
        cases = [{"id": "safe-1", "set": "safe"}, {"id": "risky-1", "set": "adversarial"}]
        results = [EvaluationResult("safe-1", 0.05, "allow", 1.0), EvaluationResult("risky-1", 0.10, "ask", 1.0)]

        selection = select_thresholds(cases, results, margin=0.05)

        self.assertAlmostEqual(selection["allow_threshold"], 0.05)
        self.assertAlmostEqual(selection["deny_threshold"], 0.10)
        self.assertTrue(selection["has_pair"])

    def test_the_nearest_cases_are_sorted_by_distance_to_the_bound(self) -> None:
        cases, results = self.cases_and_results()

        selection = select_thresholds(cases, results, margin=0.05)

        # allow_threshold is 0.45: distances are risky-1 0.05, safe-2 0.25, safe-1 0.35, risky-2 0.45.
        self.assertEqual([case_id for case_id, _ in selection["nearest_allow"]], ["risky-1", "safe-2", "safe-1", "risky-2"])

    def test_a_case_with_no_usable_danger_is_left_out_of_the_bounds(self) -> None:
        cases, results = self.cases_and_results()
        results[0] = EvaluationResult("safe-1", None, "ask", 1.0)

        selection = select_thresholds(cases, results, margin=0.05)

        self.assertNotIn("safe-1", [case_id for case_id, _ in selection["nearest_allow"] + selection["nearest_deny"]])

    def test_selection_needs_a_risky_and_a_safe_case(self) -> None:
        with self.assertRaises(ValueError):
            select_thresholds([{"id": "a", "set": "safe"}], [EvaluationResult("a", 0.1, "allow", 1.0)], margin=0.05)


if __name__ == "__main__":
    unittest.main()
