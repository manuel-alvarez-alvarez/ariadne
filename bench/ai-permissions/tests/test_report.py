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
            run.write_scores(Path(temp), 'kev_v29', self.cases, found)
            csv_text = (Path(temp) / 'kev_v29.csv').read_text()
            self.assertIn('safe', csv_text.splitlines()[0].split(','))
            self.assertNotIn('rule', csv_text.splitlines()[0].split(','))
            self.assertNotIn('cap', csv_text.splitlines()[0].split(','))
            cases, scores = run.read_scores(Path(temp) / 'kev_v29.csv')
            self.assertEqual([score.safe for score in scores], [.9, .4])
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                run.print_selection('kev_v29', cases, scores, .05)
            self.assertIn('P(safe) allow_threshold 0.4500 / deny_threshold 0.8500', output.getvalue())
            self.assertIn('nearest the allow bound:', output.getvalue())
            self.assertIn('table at that pair:', output.getvalue())

    def test_score_mode_writes_empty_safe_column(self):
        found = [EvaluationResult('safe', .1, 'allow', 1), EvaluationResult('risk', .6, 'ask', 2)]
        with tempfile.TemporaryDirectory() as temp:
            run.write_scores(Path(temp), 'kev_v28', self.cases, found)
            _, scores = run.read_scores(Path(temp) / 'kev_v28.csv')
            self.assertEqual([score.safe for score in scores], [None, None])

    def test_report_reads_legacy_csv_without_safe_column(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'old.csv'
            path.write_text('id,set,expected,danger,label,latency_ms\nsafe,safe,allow,0.1,allow,1\n')
            cases, scores = run.read_scores(path)
            self.assertEqual(cases[0]['id'], 'safe')
            self.assertIsNone(scores[0].safe)
