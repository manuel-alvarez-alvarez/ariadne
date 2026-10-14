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
    tool_block = {**record, "message": {**record["message"], "content": [{"type": "tool_use", "id": "call", "name": "Bash"}]}}
    conflict = {**record, "message": {**record["message"], "usage": {"input_tokens": 10, "cache_read_input_tokens": 4, "cache_creation_input_tokens": 2, "output_tokens": 7}}}
    result = {"type": "user", "timestamp": "2026-10-01T00:00:01Z", "sessionId": "internal", "message": {"content": [{"type": "tool_result", "tool_use_id": "call", "content": [{"type": "text", "text": "four"}, {"type": "text", "text": "chars"}]}]}}
    claude.write_text("\n".join(json.dumps(value) for value in (record, tool_block, conflict, result)) + "\n")
    values, tools, counts = followup.claude_requests(claude, {"internal": {"id": "session"}}, "2026-10-02T00:00:00Z")
    assert len(values) == 1
    assert values[0]["input_tokens"] == 16
    assert values[0]["cached_input_tokens"] == 4
    assert values[0]["tool_calls"] == 1
    assert counts["duplicate_request_records"] == 2
    assert counts["conflicting_usage_records"] == 1
    assert tools[0]["tool_name"] == "Bash"
    assert tools[0]["result_chars"] == 9

    missing_then_present = root / "missing-then-present.jsonl"
    without_usage = {**record, "requestId": "alias", "message": {"id": "alias-message", "model": "model", "content": []}}
    with_usage = {**record, "requestId": "alias", "message": {**record["message"], "id": "alias-message"}}
    missing_then_present.write_text(json.dumps(without_usage) + "\n" + json.dumps(with_usage) + "\n")
    values, _, _ = followup.claude_requests(missing_then_present, {"internal": {"id": "session"}}, "2026-10-02T00:00:00Z")
    assert len(values) == 1 and values[0]["usage_status"] == "present" and values[0]["input_tokens"] == 16

    bridge = root / "alias-bridge.jsonl"
    first = {**record, "requestId": "r", "message": {**record["message"], "id": None}}
    second = {**record, "requestId": None, "message": {**record["message"], "id": "m"}}
    third = {**record, "requestId": "r", "message": {**record["message"], "id": "m"}}
    bridge.write_text("\n".join(json.dumps(value) for value in (first, second, third)) + "\n")
    values, _, _ = followup.claude_requests(bridge, {"internal": {"id": "session"}}, "2026-10-02T00:00:00Z")
    assert len(values) == 1 and values[0]["input_tokens"] == 16

    resumed = root / "resumed.jsonl"
    resumed.write_text("\n".join(json.dumps(value) for value in (
        {"type": "token_usage_record", "timestamp": "2026-10-01T00:00:00Z", "payload": {"response_id": "same", "turn_id": "turn", "session_id": "one", "usage": {"input_tokens": 5, "cached_input_tokens": 1, "output_tokens": 1}}},
        {"type": "token_usage_record", "timestamp": "2026-10-01T00:00:01Z", "payload": {"response_id": "same", "turn_id": "turn", "session_id": "two", "usage": {"input_tokens": 7, "cached_input_tokens": 2, "output_tokens": 1}}},
        {"type": "token_usage_record", "timestamp": "2026-10-03T00:00:00Z", "payload": {"response_id": "after", "turn_id": "turn", "session_id": "one", "usage": {"input_tokens": 999, "cached_input_tokens": 0, "output_tokens": 0}}},
    )) + "\n")
    values, counts = followup.codex_requests(resumed, {"one": {"id": "first"}, "two": {"id": "second"}}, "2026-10-02T00:00:00Z")
    assert {(value["session_id"], value["request_id"]) for value in values} == {("first", "same"), ("second", "same")}
    assert counts["after_cutoff"] == 1

    codex_tool = root / "codex-tool.jsonl"
    codex_tool.write_text("\n".join(json.dumps(value) for value in (
        {"type": "session_meta", "payload": {"type": "session_meta", "id": "internal"}},
        {"type": "response_item", "payload": {"type": "custom_tool_call", "call_id": "call", "name": "shell"}},
        {"type": "response_item", "payload": {"type": "custom_tool_call_output", "call_id": "call", "output": "output"}},
    )) + "\n")
    tools, _ = followup.codex_tools(codex_tool, {"internal": {"id": "session"}}, "2026-10-02T00:00:00Z")
    assert len(tools) == 1 and tools[0]["session_id"] == "session" and tools[0]["result_chars"] == 6

print("followup parser checks: ok")
