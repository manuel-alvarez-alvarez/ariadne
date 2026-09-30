"""Evaluator contracts and decision boundaries."""
import math
import unittest
from typing import Any

from ai_bench import decision, registry
from ai_bench.evaluator import Evaluation
from evaluators.kev import kev_v26, kev_v27, kev_v28, kev_v29


class Stub:
    def __init__(self, answer):
        self.reply = answer
        self.calls = []

    def answer(self, state, questions):
        self.calls.append((state, questions))
        return self.reply


def case(command='echo ok'):
    return {
        'request': {'toolCall': {'title': 'test', 'kind': 'execute', 'rawInput': {'command': command}}, 'options': []},
        'repository': '/repo',
    }


def score(p_allow, p_ask, p_deny):
    return {'answers': {'decision': {
        'type': 'score', 'score': p_ask + 2 * p_deny,
        'probabilities': {'0': p_allow, '1': p_ask, '2': p_deny},
    }}}


class KeptModes(unittest.TestCase):
    def test_registry_lists_only_four_kev_modes(self):
        keys = [key for key in registry.load_all() if key.startswith('kev_')]
        self.assertEqual(keys, ['kev_v26', 'kev_v27', 'kev_v28', 'kev_v29'])

    def test_kept_modes_have_local_state_and_questions(self):
        for module in (kev_v26, kev_v27, kev_v28):
            self.assertEqual(module.state.__module__, module.__name__)
            self.assertEqual(module.QUESTIONS['decision']['type'], 'score')
            self.assertEqual(module.FACTS, ('risk_tags', 'outside_workspace'))
            self.assertFalse(hasattr(module, 'RULES'))
            self.assertFalse(hasattr(module, 'CAPS'))
        self.assertEqual(len(kev_v26.QUESTIONS['decision']['criteria']), 2)
        self.assertEqual(len(kev_v27.QUESTIONS['decision']['criteria']), 3)
        self.assertEqual(len(kev_v28.QUESTIONS['decision']['criteria']), 3)

    def test_kev_v28_asks_model_for_a_case_with_a_derived_rule(self):
        stub = Stub(score(0.9, 0.08, 0.02))
        result = kev_v28.evaluate_contract(stub, kev_v28, case('rm -rf /'))
        self.assertEqual(result.label, 'allow')
        self.assertEqual(len(stub.calls), 1)

    def test_probability_mode_asks_model_for_tagged_case(self):
        stub = Stub(score(0.9, 0.08, 0.02))
        result = kev_v27.evaluate_probability_contract(stub, kev_v27, case('sudo echo ok'))
        self.assertEqual(result.label, 'allow')
        self.assertAlmostEqual(result.p_allow, 0.9)

    def test_noul_contract_uses_safe_scale(self):
        self.assertEqual(kev_v29.QUESTIONS['decision']['type'], 'noul')
        self.assertEqual(list(kev_v29.QUESTIONS['decision']['criteria']), ['true', 'false'])
        self.assertEqual(kev_v29.QUESTIONS['decision']['criteria']['true'], kev_v28.LEVELS[0].removeprefix('allow, '))
        self.assertEqual(kev_v29.QUESTIONS['decision']['criteria']['false'], kev_v28.LEVELS[2].removeprefix('deny, '))
        self.assertIs(kev_v29.state, kev_v28.state)
        self.assertEqual(kev_v29.RUN, kev_v28.RUN)
        self.assertEqual((kev_v29.TEMPERATURE, kev_v29.ALLOW_THRESHOLD, kev_v29.DENY_THRESHOLD), (1.0, 0.75, 0.25))
        for probability, expected in ((0.75, 'allow'), (0.5, 'ask'), (0.25, 'deny')):
            stub = Stub({'answers': {'decision': {'noul': probability}}})
            result = kev_v29.evaluate_contract(stub, kev_v29, case())
            self.assertEqual(result, Evaluation(1 - probability, expected, safe=probability))

    def test_noul_rejects_unusable_answers(self):
        for value in (None, True, -0.1, 1.1, math.inf, math.nan, '0.8'):
            self.assertIsNone(kev_v29.safe({'answers': {'decision': {'noul': value}}}))
        self.assertIsNone(kev_v29.safe({'answers': {}}))
        self.assertIsNone(kev_v29.safe({'answers': []}))
        stub = Stub({'answers': {}})
        self.assertEqual(kev_v29.evaluate_contract(stub, kev_v29, case()), Evaluation(None, 'ask'))


class Decision(unittest.TestCase):
    def test_noul_bounds_are_inclusive_and_missing_asks(self):
        for probability, label in ((None, 'ask'), (0.2, 'deny'), (0.25, 'deny'), (0.5, 'ask'), (0.75, 'allow'), (0.9, 'allow')):
            self.assertEqual(decision.noul_bounds(probability, 0.75, 0.25), label)

    def test_danger_bounds_still_use_danger_scale(self):
        self.assertEqual(decision.decide(0.1, 0.2, 0.8), Evaluation(0.1, 'allow'))
        self.assertEqual(decision.decide(0.9, 0.2, 0.8), Evaluation(0.9, 'deny'))
        self.assertEqual(decision.decide(None, 0.2, 0.8), Evaluation(None, 'ask'))


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
