#!/usr/bin/env python3
"""The orchestrator agent both README demos register and run.

A scriptable stub ACP agent, in python3 with only the standard library, built
on the shape of the test stub at
crates/ariadne-daemon/tests/it/common/acp.rs. It speaks ACP version 1 over its
standard input and output and plays one fixed, two-turn conversation: the
first session/prompt (the daemon's own initial prompt once the orchestrator
session starts) reads a file of the demo repository and asks a short
question about the rate limit; the second (the answer the person types into
the console, forwarded as the next session/prompt) accepts it. Both README
recordings (assets/demo.tape and assets/record-demo-ui.mjs) register this
same script as the orchestrator's agent, so the conversation they show is
identical. Neither recording's two tasks come from this agent — a real
orchestrator would propose them over the same ACP turn this plays short, but
assets/seed-demo-goal.py adds them and finalizes the plan over the daemon's
REST API instead, the way both recordings already shared that step before
this agent existed.

The daemon starts this process directly as the registered command (`python3
demo-orchestrator-agent.py`, no arguments) and holds the child with
`kill_on_drop`, so stopping the daemon ends it; this script does not need to
watch for an orphaned parent itself.
"""

import json
import sys

SESSION_ID = "demo-orchestrator-session"
MODEL_OPTION = {
    "id": "model-id",
    "name": "model-id",
    "category": "model",
    "type": "select",
    "currentValue": "demo-model",
    "options": [],
}
TURNS = [
    {
        "updates": [
            {
                "sessionUpdate": "tool_call",
                "toolCallId": "call-1",
                "title": "Read",
                "rawInput": {"path": "package.json"},
            },
            {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call-1",
                "title": "Read",
                "status": "completed",
            },
            {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": "Quick check before I plan this: should the "
                    "limit apply per IP address, or per account?",
                },
            },
        ],
    },
    {
        "updates": [
            {
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "type": "text",
                    "text": "Got it — I'll scope the limiter per IP "
                    "address and draft the plan now.",
                },
            },
        ],
    },
]


def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def send_updates(updates):
    for update in updates:
        send(
            {
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {"sessionId": SESSION_ID, "update": update},
            }
        )


def respond(request):
    method = request.get("method")
    if method == "initialize":
        return {"protocolVersion": 1, "agentCapabilities": {}}
    if method == "session/new":
        return {"sessionId": SESSION_ID, "configOptions": [MODEL_OPTION]}
    if method == "session/set_config_option":
        params = request["params"]
        if MODEL_OPTION["id"] == params.get("configId"):
            MODEL_OPTION["currentValue"] = params.get("value")
        return {"configOptions": [MODEL_OPTION]}
    if method == "session/close":
        return {}
    if method == "session/prompt":
        turn = TURNS.pop(0) if TURNS else {}
        send_updates(turn.get("updates", []))
        return {"stopReason": "end_turn"}
    raise Failure(-32601, "method not supported: %s" % method)


class Failure(Exception):
    def __init__(self, code, message):
        self.code, self.message = code, message


while True:
    line = sys.stdin.readline()
    if not line:
        break
    message = json.loads(line)
    if "method" not in message or "id" not in message:
        continue  # a response to a request this agent never sends
    try:
        send({"jsonrpc": "2.0", "id": message["id"], "result": respond(message)})
    except Failure as failure:
        send(
            {
                "jsonrpc": "2.0",
                "id": message["id"],
                "error": {"code": failure.code, "message": failure.message},
            }
        )
