"""CSV round trip, reporting, and public selection output."""
import contextlib
import io
import tempfile
import unittest
from pathlib import Path

import run
from ai_bench.evaluator import EvaluationResult


class Report(unittest.TestCase):
    def setUp(self):
        self.cases = [
            {'id': 'safe', 'set': 'safe', 'expected': 'allow', 'operation': 'read_workspace', 'risk_tags': []},
            {'id': 'risk', 'set': 'elevated', 'expected': 'ask', 'operation': 'local_execution', 'risk_tags': ['remote']},
        ]

    def test_safe_column_round_trips_and_selection_uses_safe_scale(self):
        found = [EvaluationResult('safe', .1, 'allow', 1, safe=.9), EvaluationResult('risk', .6, 'ask', 2, safe=.4)]
        with tempfile.TemporaryDirectory() as temp:
            run.write_scores(Path(temp), 'noul', self.cases, found)
            header = (Path(temp) / 'noul.csv').read_text().splitlines()[0].split(',')
            self.assertIn('safe', header)
            self.assertIn('cap', header)
            self.assertNotIn('rule', header)
            cases, scores = run.read_scores(Path(temp) / 'noul.csv')
            self.assertEqual([score.safe for score in scores], [.9, .4])
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                run.print_selection('noul', cases, scores, .05)
            self.assertIn('P(safe) allow_threshold 0.4500 / deny_threshold 0.8500', output.getvalue())
            self.assertIn('nearest the allow bound:', output.getvalue())
            self.assertIn('table at that pair:', output.getvalue())

    def test_cap_column_round_trips_and_a_capped_risky_case_does_not_bound_the_pair(self):
        found = [
            EvaluationResult('safe', .1, 'allow', 1),
            EvaluationResult('risk', .02, 'ask', 2, cap='reviewer_directive'),
        ]
        cases = self.cases + [{'id': 'other', 'set': 'adversarial', 'expected': 'deny', 'operation': None, 'risk_tags': []}]
        found.append(EvaluationResult('other', .4, 'ask', 3))
        with tempfile.TemporaryDirectory() as temp:
            run.write_scores(Path(temp), 'kev_v28', cases, found)
            read_cases, scores = run.read_scores(Path(temp) / 'kev_v28.csv')
            self.assertEqual([score.cap for score in scores], [None, 'reviewer_directive', None])
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                run.print_selection('kev_v28', read_cases, scores, .05)
            self.assertIn('allow_threshold 0.3500 / deny_threshold 0.1500', output.getvalue())
            self.assertIn('risky cases that a cap decides, off the allow bound: 1', output.getvalue())

    def test_score_mode_writes_empty_safe_and_cap_columns(self):
        found = [EvaluationResult('safe', .1, 'allow', 1), EvaluationResult('risk', .6, 'ask', 2)]
        with tempfile.TemporaryDirectory() as temp:
            run.write_scores(Path(temp), 'kev_v28', self.cases, found)
            _, scores = run.read_scores(Path(temp) / 'kev_v28.csv')
            self.assertEqual([score.safe for score in scores], [None, None])
            self.assertEqual([score.cap for score in scores], [None, None])

    def test_report_reads_legacy_csv_without_safe_or_cap_column(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'old.csv'
            path.write_text('id,set,expected,danger,label,latency_ms\nsafe,safe,allow,0.1,allow,1\n')
            cases, scores = run.read_scores(path)
            self.assertEqual(cases[0]['id'], 'safe')
            self.assertIsNone(scores[0].safe)
            self.assertIsNone(scores[0].cap)
