import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parents[1]))
import experiment

class FailureExperimentTests(unittest.TestCase):
    def test_baseline_matches_protocol_and_patterns(self):
        self.assertTrue(experiment.baseline({"message": "RATE LIMIT", "data": {}}))
        self.assertTrue(experiment.baseline({"message": "other", "data": {"codexErrorInfo": "usageLimitExceeded"}}))
        self.assertFalse(experiment.baseline({"message": "other", "data": {}}))
    def test_rules_ignore_negation_and_quotes(self):
        self.assertIsNone(experiment.rules({"message": "This is not a rate limit", "data": {}}))
        self.assertIsNone(experiment.rules({"message": "quoted error: rate limit", "data": {}}))
    def test_rules_preserve_protocol_exhaustion(self):
        self.assertEqual(experiment.rules({"message": "not a rate limit", "data": {"codexErrorInfo": "usageLimitExceeded"}}), "exhausted")
    def test_kev_failure_uses_baseline_fallback(self):
        label, source = experiment.policy({"message": "rate limit", "data": {}}, None, "rules_first_kev")
        self.assertEqual((label, source), ("exhausted", "baseline_fallback"))
    def test_same_family_has_one_split(self):
        cases = experiment.read_cases(Path(__file__).parents[1] / "cases.jsonl")
        splits = {}
        for case in cases: splits.setdefault(case["source_family"], set()).add(case["split"])
        self.assertTrue(all(len(found) == 1 for found in splits.values()))
    def test_parser_reads_the_frozen_corpus(self):
        self.assertEqual(len(experiment.read_cases(Path(__file__).parents[1] / "cases.jsonl")), 125)
    def test_metrics_count_missed_exhaustion_and_wrong_switches(self):
        rows = [{"label": "exhausted", "recovery": "switch_model", "policies": {"x": {"label": "insufficient", "recovery": "abstain"}}}, {"label": "task_error", "recovery": "task_action", "policies": {"x": {"label": "exhausted", "recovery": "switch_model"}}}]
        found = experiment.metric(rows, "x")
        self.assertEqual((found["missed_exhaustion"], found["wrong_switch_recommendations"]), (1, 1))

if __name__ == "__main__": unittest.main()
