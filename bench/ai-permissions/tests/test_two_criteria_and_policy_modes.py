"""The three retained score contracts and the probability policy."""
import unittest

from ai_bench import metrics
from ai_bench.evaluator import EvaluationResult
from evaluators.kev import kev_v26, kev_v27, kev_v28


def answer(p_allow, p_ask, p_deny):
    return {'answers': {'decision': {'score': p_ask + 2*p_deny,
                                     'probabilities': {'0': p_allow, '1': p_ask, '2': p_deny}}}}


class ScoreContracts(unittest.TestCase):
    def test_two_criteria_mode_uses_deny_probability_as_danger(self):
        value = {'answers': {'decision': {'score': .3, 'probabilities': {'0': .7, '1': .3}}}}
        self.assertAlmostEqual(kev_v26.danger(value), .3)
        self.assertEqual(kev_v26.QUESTIONS['decision']['criteria'], [kev_v27.LEVELS[0], kev_v27.LEVELS[2]])

    def test_probability_mode_asks_on_both_bounds(self):
        original_allow, original_deny = kev_v27.ALLOW_PROBABILITY, kev_v27.DENY_PROBABILITY
        try:
            kev_v27.ALLOW_PROBABILITY = .4
            kev_v27.DENY_PROBABILITY = .4
            self.assertEqual(kev_v27.decision(answer(.5, 0, .5)).label, 'ask')
        finally:
            kev_v27.ALLOW_PROBABILITY, kev_v27.DENY_PROBABILITY = original_allow, original_deny

    def test_probability_mode_allows_without_cap(self):
        found = kev_v27.decision(answer(.9, .08, .02))
        self.assertEqual(found.label, 'allow')
        self.assertAlmostEqual(found.p_allow, .9)

    def test_selection_of_probability_mode_uses_both_probabilities(self):
        cases = [{'id': 'safe', 'set': 'safe'}, {'id': 'risky', 'set': 'adversarial'}]
        results = [EvaluationResult('safe', .2, 'allow', 1, p_allow=.8, p_deny=.1),
                   EvaluationResult('risky', .8, 'deny', 1, p_allow=.2, p_deny=.7)]
        pair = metrics.select_probability_thresholds(cases, results, .05)
        self.assertAlmostEqual(pair['allow_probability'], .25)
        self.assertAlmostEqual(pair['deny_probability'], .15)
        self.assertEqual(metrics.at_probability_thresholds(results[0], .25, .15).label, 'allow')

    def test_kev_v28_adds_outside_read_to_allow_criterion(self):
        self.assertIn('reading, listing and searching files outside the workspace', kev_v28.LEVELS[0])
        self.assertNotIn('reading, listing and searching files outside the workspace', kev_v27.LEVELS[0])
        self.assertEqual(kev_v28.LEVELS[1:], kev_v27.LEVELS[1:])
