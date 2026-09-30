"""Probe probabilities and offline temperature on both scales."""
import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import run
from ai_bench import probe
from evaluators.kev import kev_v29


class Prober:
    def __init__(self):
        self.requests = []

    def probabilities(self, state, questions):
        self.requests.append((state, questions))
        return {name: [0.2, 0.8] for name in questions}


def record(name, group, p):
    return {
        'id': name, 'set': group, 'expected': 'allow' if group == 'safe' else 'ask',
        'operation': None, 'risk_tags': [], 'pair': None,
        'derived': {'operation_hint': None, 'risk_tags': []},
        'probabilities': {'kev_v29': {'decision': [1-p, p]}},
    }


class Probe(unittest.TestCase):
    def test_probe_records_both_noul_probabilities(self):
        case = {
            'id': 'one', 'set': 'safe', 'expected': 'allow',
            'request': {'toolCall': {'title': 'read', 'kind': 'execute', 'rawInput': {'command': 'cat README.md'}}, 'options': []},
            'repository': '/repo',
        }
        prober = Prober()
        [found] = run.probe_cases(prober, [('kev_v29', kev_v29)], [case])
        self.assertEqual(found['probabilities']['kev_v29']['decision'], [0.2, 0.8])
        self.assertEqual(set(found['derived']), {'operation_hint', 'risk_tags'})
        self.assertEqual(list(prober.requests[0][1]), ['kev_v29.decision'])

    def test_offline_noul_temperature_uses_binary_logit(self):
        values = probe.at_temperature([.2, .8], 2.0)
        self.assertAlmostEqual(values[1], 2/3)
        answer = probe.answers(kev_v29.QUESTIONS, {'decision': [.2, .8]}, 2.0)
        self.assertAlmostEqual(kev_v29.safe(answer), 2/3)
        self.assertEqual(kev_v29.safe(probe.answers(kev_v29.QUESTIONS, {"decision": [1.0, 0.0]}, 2.0)), 0.0)
        self.assertEqual(kev_v29.safe(probe.answers(kev_v29.QUESTIONS, {"decision": [0.0, 1.0]}, 2.0)), 1.0)

    def test_measure_temperature_uses_safe_pair(self):
        records = [record('safe', 'safe', .9), record('risky', 'elevated', .4)]
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'probe.jsonl'
            path.write_text(''.join(json.dumps(item) + '\n' for item in records))
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(run.main(['measure', 'temperature', str(path), '--evaluator', 'kev_v29', '--temperature', '2', '--pair', '.75', '.25']), 0)
            self.assertIn('allow P(safe)', output.getvalue())
            self.assertIn('deny P(safe)', output.getvalue())
            self.assertIn('0.7500', output.getvalue())

    def test_results_store_safe_and_inverse_danger(self):
        found = probe.results([record('safe', 'safe', .8)], 'kev_v29', kev_v29.QUESTIONS, kev_v29.safe, 1.0, True)
        self.assertAlmostEqual(found[0].safe, .8)
        self.assertAlmostEqual(found[0].danger, .2)
