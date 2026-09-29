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
            {"id": "b", "set": "elevated", "expected": "ask"},
            {"id": "c", "set": "adversarial", "expected": "deny"},
            {"id": "d", "set": "real", "expected": "allow"},
        ]
        results = [
            EvaluationResult("a", 0.1, "allow", 3.5),
            EvaluationResult("b", 0.5, "ask", 4.0),
            EvaluationResult("c", None, "ask", 0.0),
            EvaluationResult("d", 0.3, "allow", 5.25),
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v1", cases, results)
            [path] = run.score_files([directory])
            read_cases, read_results = run.read_scores(path)

        self.assertEqual(read_results, results)
        self.assertEqual(metrics.summary(read_cases, read_results), metrics.summary(cases, results))

    def test_report_prints_one_row_per_score_file(self) -> None:
        cases = [{"id": "a", "set": "safe", "expected": "allow"}]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "laya_v1", cases, [EvaluationResult("a", 0.1, "allow", 1.0)])
            run.write_scores(Path(directory), "kev_v1", cases, [EvaluationResult("a", 0.9, "deny", 2.0)])
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["report", directory])

        self.assertEqual(code, 0)
        rows = output.getvalue().splitlines()[2:]
        self.assertEqual([row.split()[0] for row in rows], ["kev_v1", "laya_v1"])

    def test_heldout_replaces_the_development_cases_or_follows_named_ones(self) -> None:
        self.assertEqual(run.run_case_files(None), run.DEFAULT_CASES)
        self.assertEqual(run.run_case_files(None, heldout=True), run.HELDOUT_CASES)
        named = str(run.HERE / "cases")
        files = run.run_case_files([named], heldout=True)
        self.assertEqual(files[-2:], run.HELDOUT_CASES)
        self.assertFalse(any(f.name.endswith("-heldout.jsonl") for f in files[:-2]), files)

    def test_a_csv_without_the_new_columns_still_reports(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "kev_v1.csv"
            path.write_text("id,set,expected,danger,label,latency_ms\na,safe,allow,0.1,allow,1.0\n", encoding="utf-8")
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["report", str(path)])

        self.assertEqual(code, 0)
        self.assertIn("kev_v1", output.getvalue())

    def test_a_score_file_keeps_the_rule_and_the_cap_of_each_case(self) -> None:
        cases = [
            {"id": "a", "set": "adversarial", "expected": "deny"},
            {"id": "b", "set": "elevated", "expected": "ask"},
            {"id": "c", "set": "safe", "expected": "allow"},
        ]
        results = [
            EvaluationResult("a", None, "deny", 0.0, rule="root_delete"),
            EvaluationResult("b", 0.1, "ask", 4.0, cap="force"),
            EvaluationResult("c", 0.1, "allow", 4.0),
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v1", cases, results)
            [path] = run.score_files([directory])
            _, read_results = run.read_scores(path)

        self.assertEqual(read_results, results)

    def test_select_leaves_a_capped_risky_case_off_the_allow_bound(self) -> None:
        cases = [
            {"id": "safe-1", "set": "safe", "expected": "allow"},
            {"id": "risky-1", "set": "elevated", "expected": "ask"},
            {"id": "risky-2", "set": "adversarial", "expected": "deny"},
        ]
        results = [
            EvaluationResult("safe-1", 0.30, "ask", 1.0),
            EvaluationResult("risky-1", 0.10, "ask", 1.0, cap="force"),
            EvaluationResult("risky-2", 0.38, "ask", 1.0),
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v1", cases, results)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["select", directory, "--margin", "0.05"])

        self.assertEqual(code, 0)
        text = output.getvalue()
        self.assertIn("  pair: allow_threshold 0.3300 / deny_threshold 0.3500", text)
        self.assertIn("risky cases that a rule or a cap decides, off the allow bound: 1", text)

    def test_select_names_a_safe_case_that_a_rule_denies(self) -> None:
        cases = [
            {"id": "safe-1", "set": "safe", "expected": "allow"},
            {"id": "safe-2", "set": "safe", "expected": "allow"},
            {"id": "risky-1", "set": "adversarial", "expected": "deny"},
        ]
        results = [
            EvaluationResult("safe-1", 0.05, "allow", 1.0),
            EvaluationResult("safe-2", None, "deny", 0.0, rule="home_delete"),
            EvaluationResult("risky-1", 0.50, "ask", 1.0),
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v1", cases, results)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["select", directory, "--margin", "0.05"])

        self.assertEqual(code, 0)
        text = output.getvalue()
        self.assertIn("broken hard rule, a rule denies 1 safe or real case(s):", text)
        self.assertIn("    safe-2", text)
        # The table at the pair keeps the deny of the rule.
        self.assertEqual(text.splitlines()[-1].split()[4:6], ["0", "1"])

    def test_report_by_operation_prints_one_row_per_operation(self) -> None:
        cases = [
            {"id": "a", "set": "safe", "expected": "allow", "operation": "read_workspace"},
            {"id": "b", "set": "adversarial", "expected": "deny", "operation": "network_read"},
        ]
        results = [EvaluationResult("a", 0.1, "allow", 1.0), EvaluationResult("b", 0.9, "deny", 1.0)]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v1", cases, results)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["report", directory, "--by", "operation"])

        self.assertEqual(code, 0)
        text = output.getvalue()
        self.assertIn("read_workspace", text)
        self.assertIn("network_read", text)

    def test_report_by_pair_names_the_incorrect_pair(self) -> None:
        cases = [
            {"id": "safe-1", "set": "safe", "expected": "allow", "pair": "adversarial-1"},
            {"id": "adversarial-1", "set": "adversarial", "expected": "deny", "pair": "safe-1"},
        ]
        results = [
            EvaluationResult("safe-1", 0.1, "allow", 1.0),
            EvaluationResult("adversarial-1", 0.1, "allow", 1.0),  # wrong: expected deny
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(Path(directory), "kev_v1", cases, results)
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["report", directory, "--by", "pair"])

        self.assertEqual(code, 0)
        text = output.getvalue()
        self.assertIn("pairs: 1", text)
        self.assertIn("correct: 0", text)
        self.assertIn("adversarial-1 / safe-1", text)

    def test_report_fails_on_an_empty_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stderr(io.StringIO()) as error:
            code = run.main(["report", directory])

        self.assertEqual(code, 1)
        self.assertIn("no score files", error.getvalue())

    def test_select_prints_a_pair_and_the_table_per_evaluator(self) -> None:
        cases = [
            {"id": "safe-1", "set": "safe", "expected": "allow"},
            {"id": "risky-1", "set": "adversarial", "expected": "deny"},
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(
                Path(directory),
                "kev_v1",
                cases,
                [EvaluationResult("safe-1", 0.05, "allow", 1.0), EvaluationResult("risky-1", 0.10, "ask", 1.0)],
            )
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["select", directory, "--margin", "0.05"])

        self.assertEqual(code, 0)
        self.assertIn("pair: allow_threshold 0.0500 / deny_threshold 0.1000", output.getvalue())

    def test_select_reports_no_pair_when_the_bounds_cross(self) -> None:
        cases = [
            {"id": "safe-1", "set": "safe", "expected": "allow"},
            {"id": "risky-1", "set": "adversarial", "expected": "deny"},
        ]
        with tempfile.TemporaryDirectory() as directory:
            run.write_scores(
                Path(directory),
                "kev_v1",
                cases,
                [EvaluationResult("safe-1", 0.1, "allow", 1.0), EvaluationResult("risky-1", 0.9, "deny", 1.0)],
            )
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                code = run.main(["select", directory, "--margin", "0.05"])

        self.assertEqual(code, 0)
        self.assertIn("no pair", output.getvalue())


if __name__ == "__main__":
    unittest.main()
