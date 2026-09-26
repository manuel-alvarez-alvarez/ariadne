from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ai_bench.evaluator import EvaluationResult
from ai_bench.metrics import auroc, classification_metrics, coverage


class MetricsTests(unittest.TestCase):
    def test_classification_metrics_treat_allow_as_the_positive_class(self) -> None:
        metrics = classification_metrics(
            expected=["allow", "allow", "escalate", "escalate"],
            labels=["allow", "escalate", "allow", "escalate"],
        )

        self.assertEqual(metrics["precision"], 0.5)
        self.assertEqual(metrics["recall"], 0.5)
        self.assertEqual(metrics["f1"], 0.5)
        self.assertEqual(metrics["accuracy"], 0.5)

    def test_auroc_averages_tied_scores(self) -> None:
        self.assertEqual(auroc([0.9, 0.5, 0.5, 0.1], [True, True, False, False]), 0.875)

    def test_coverage_counts_allowed_results(self) -> None:
        results = [
            EvaluationResult("a", 0.9, "allow", None, 3.0),
            EvaluationResult("b", 0.2, "escalate", None, 4.0),
            EvaluationResult("c", None, "escalate", "credential-paths", 0.0),
        ]

        self.assertEqual(coverage(results), 1 / 3)


if __name__ == "__main__":
    unittest.main()
