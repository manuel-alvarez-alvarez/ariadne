from __future__ import annotations

import json
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ai_bench.evaluator import AriadneEvaluator


def case(case_id: str) -> dict:
    return {
        "id": case_id,
        "set": "safe",
        "expected": "allow",
        "category": "test",
        "note": "test case",
        "repository": "/repo",
        "request": {
            "toolCall": {
                "toolCallId": case_id,
                "name": "Bash",
                "title": "git status",
                "kind": "execute",
                "rawInput": {"command": "git status"},
                "locations": [],
            },
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}],
        },
    }


class AriadneEvaluatorTests(unittest.TestCase):
    def test_parses_one_ordered_result_for_each_case_from_the_command(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            stub = Path(directory) / "stub.py"
            stub.write_text(
                textwrap.dedent(
                    """\
                    import json
                    import sys

                    if sys.argv[1:] != ["--endpoint", "http://kev.test", "--threshold", "0.6"]:
                        raise SystemExit("missing evaluator configuration")

                    for line in sys.stdin:
                        case = json.loads(line)
                        print(json.dumps({
                            "id": case["id"],
                            "allow_score": 0.75 if case["id"] == "first" else None,
                            "label": "allow" if case["id"] == "first" else "escalate",
                            "guardrail": None if case["id"] == "first" else "credential-paths",
                            "latency_ms": 12.5,
                        }))
                    """
                ),
                encoding="utf-8",
            )

            results = AriadneEvaluator([sys.executable, str(stub)]).evaluate(
                {"endpoint": "http://kev.test", "threshold": 0.6}, [case("first"), case("second")]
            )

        self.assertEqual([result.id for result in results], ["first", "second"])
        self.assertEqual(results[0].allow_score, 0.75)
        self.assertEqual(results[0].label, "allow")
        self.assertIsNone(results[1].allow_score)
        self.assertEqual(results[1].guardrail, "credential-paths")


if __name__ == "__main__":
    unittest.main()
