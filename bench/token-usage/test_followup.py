#!/usr/bin/env python3
"""Focused parser checks for the request-level transcript follow-up."""

import importlib.util
import json
import tempfile
from pathlib import Path


spec = importlib.util.spec_from_file_location("followup", Path(__file__).with_name("followup.py"))
followup = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(followup)


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    codex = root / "rollout.jsonl"
    codex.write_text("\n".join(json.dumps(value) for value in (
        {"type": "token_usage_record", "timestamp": "2026-10-01T00:00:00Z", "payload": {"response_id": "r1", "turn_id": "shared", "session_id": "internal", "usage": {"input_tokens": 11, "cached_input_tokens": 7, "output_tokens": 2}, "turn_token_usage": {"input_tokens": 99}, "thread_token_usage": {"input_tokens": 999}}},
        {"type": "token_usage_record", "timestamp": "2026-10-01T00:00:01Z", "payload": {"response_id": "r2", "turn_id": "shared", "session_id": "internal", "usage": {"input_tokens": 13, "cached_input_tokens": 8, "output_tokens": 3}, "turn_token_usage": {"input_tokens": 88}, "thread_token_usage": {"input_tokens": 888}}},
    )) + "\n")
    values, counts = followup.codex_requests(codex, {"internal": {"id": "session"}}, "2026-10-02T00:00:00Z")
    assert [value["request_id"] for value in values] == ["r1", "r2"]
    assert sum(value["input_tokens"] for value in values) == 24
    assert counts["request_usage_records"] == 2

    claude = root / "claude.jsonl"
    record = {"type": "assistant", "timestamp": "2026-10-01T00:00:00Z", "sessionId": "internal", "requestId": "request", "uuid": "turn", "message": {"id": "message", "model": "model", "usage": {"input_tokens": 10, "cache_read_input_tokens": 4, "cache_creation_input_tokens": 2, "output_tokens": 3}, "content": []}}
    claude.write_text(json.dumps(record) + "\n" + json.dumps(record) + "\n")
    values, _, counts = followup.claude_requests(claude, {"internal": {"id": "session"}}, "2026-10-02T00:00:00Z")
    assert len(values) == 1
    assert values[0]["input_tokens"] == 16
    assert values[0]["cached_input_tokens"] == 4
    assert counts["duplicate_request_records"] == 1

print("followup parser checks: ok")
