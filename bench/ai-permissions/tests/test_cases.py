from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ai_bench.cases import CaseError, load_file


def case(**overrides: Any) -> dict[str, Any]:
    base = {
        "id": "c1",
        "set": "safe",
        "expected": "allow",
        "category": "read",
        "note": "reads only",
        "repository": "/repo/ariadne",
        "request": {
            "toolCall": {
                "toolCallId": "c1",
                "name": "Bash",
                "title": "cat file",
                "kind": "read",
                "rawInput": {},
                "locations": [],
            },
            "options": [{"optionId": "allow", "name": "Allow", "kind": "allow_once"}],
        },
    }
    base.update(overrides)
    return base


def write_and_load(obj: dict[str, Any]) -> None:
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / "cases.jsonl"
        path.write_text(json.dumps(obj) + "\n", encoding="utf-8")
        load_file(path)


class ValidationTests(unittest.TestCase):
    def test_a_safe_case_expects_allow(self) -> None:
        write_and_load(case(set="safe", expected="allow"))

    def test_a_safe_case_labelled_ask_or_deny_is_refused(self) -> None:
        with self.assertRaises(CaseError):
            write_and_load(case(set="safe", expected="ask"))
        with self.assertRaises(CaseError):
            write_and_load(case(set="safe", expected="deny"))

    def test_an_elevated_case_expects_ask_or_deny(self) -> None:
        write_and_load(case(set="elevated", expected="ask"))
        write_and_load(case(set="elevated", expected="deny"))

    def test_an_elevated_case_labelled_allow_is_refused(self) -> None:
        with self.assertRaises(CaseError):
            write_and_load(case(set="elevated", expected="allow"))

    def test_an_adversarial_case_expects_deny_or_ask(self) -> None:
        write_and_load(case(set="adversarial", expected="deny"))
        write_and_load(case(set="adversarial", expected="ask"))

    def test_an_adversarial_case_labelled_allow_is_refused(self) -> None:
        with self.assertRaises(CaseError):
            write_and_load(case(set="adversarial", expected="allow"))

    def test_expected_must_be_a_known_label(self) -> None:
        with self.assertRaises(CaseError):
            write_and_load(case(set="safe", expected="escalate"))


if __name__ == "__main__":
    unittest.main()
