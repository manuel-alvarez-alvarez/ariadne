from __future__ import annotations

import contextlib
import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import run
from ai_bench import metrics
from ai_bench.evaluator import EvaluationResult


class ReportTests(unittest.TestCase):
    def test_a_written_score_file_reads_back_to_the_same_summary(self) -> None:
        cases = [
            {"id": "a", "set": "safe", "expected": "allow"},
            {"id": "b", "set": "elevated", "expected": "escalate"},
            {"id": "c", "set": "adversarial", "expected": "escalate"},
            {"id": "d", "set": "real", "expected": "allow"},
        ]
        results = [
            EvaluationResult("a", 0.9, "allow", 3.5),
            EvaluationResult("b", 0.2, "escalate", 4.0),
            EvaluationResult("c", None, "escalate", 0.0),
            EvaluationResult("d", 0.7, "allow", 5.25),
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev=configs/kev.json", cases, results)
            [path] = run.score_files([directory])
            read_cases, read_results = run.read_scores(path)

        self.assertEqual(read_results, results)
        self.assertEqual(metrics.summary(read_cases, read_results), metrics.summary(cases, results))

    def test_report_prints_one_row_per_score_file(self) -> None:
        cases = [{"id": "a", "set": "safe", "expected": "allow"}]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "laya", cases, [EvaluationResult("a", 0.9, "allow", 1.0)])
            run.write_scores(Path(directory), "kev", cases, [EvaluationResult("a", 0.1, "escalate", 2.0)])
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["report", directory])

        self.assertEqual(code, 0)
        rows = output.getvalue().splitlines()[2:]
        self.assertEqual([row.split()[0] for row in rows], ["kev", "laya"])

    def test_report_fails_on_an_empty_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stderr(io.StringIO()) as error:
            code = run.main(["report", directory])

        self.assertEqual(code, 1)
        self.assertIn("no score files", error.getvalue())


if __name__ == "__main__":
    unittest.main()
