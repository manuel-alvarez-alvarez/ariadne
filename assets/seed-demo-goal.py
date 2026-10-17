#!/usr/bin/env python3
"""Seed the README demo goal's tasks over the daemon's REST API.

Shared by both README demo recordings, so they show the same tasks:
assets/demo.tape (vhs) types `ariadne goal create`, and
assets/record-demo-ui.mjs (Playwright) fills the New goal form. Each creates
the goal itself through its own user-facing path, titled exactly
TITLE below; this script finds that goal, adds the same two tasks, and
finalizes the plan, the way a real orchestrator would once the user agreed
to it.

Usage: seed-demo-goal.py <base-url> <model>
Prints the goal id, then the first task's id, one per line.
"""

import json
import sys
import time
import urllib.request

TITLE = "Add rate limiting to the login endpoint"
STEPS = ["develop", "review", "merge"]


def call(base_url, method, path, body=None, session=None):
    request = urllib.request.Request(f"{base_url}{path}", method=method)
    if body is not None:
        request.data = json.dumps(body).encode()
        request.add_header("Content-Type", "application/json")
    if session is not None:
        request.add_header("X-Ariadne-Session", session)
    with urllib.request.urlopen(request) as response:
        raw = response.read()
        return json.loads(raw) if raw else None


def agents(model):
    return [{"step": step, "model": model} for step in STEPS]


def wait_for(check, description, timeout=20.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.15)
    raise TimeoutError(f"timed out waiting for {description}")


def main():
    base_url, model = sys.argv[1:3]

    # Started in the background before the goal exists (assets/demo.tape
    # launches it ahead of the typed `ariadne goal create`, so nothing it
    # prints reaches the recording), so the goal itself is waited for too —
    # generously, since vhs still has to finish typing out both commands at
    # its own recorded pace before the goal create one even runs.
    goal = wait_for(
        lambda: next(
            (g for g in call(base_url, "GET", "/v1/goals") if g["title"] == TITLE),
            None,
        ),
        "the demo goal",
        timeout=60.0,
    )

    limiter = call(
        base_url,
        "POST",
        f"/v1/goals/{goal['id']}/tasks",
        {
            "title": "Add the rate limiter middleware",
            "description": "Count login attempts per IP address in a fixed "
            "one-minute window and reject the rest with 429.",
            "agents": agents(model),
        },
    )
    call(
        base_url,
        "POST",
        f"/v1/goals/{goal['id']}/tasks",
        {
            "title": "Test the rate limit",
            "description": "Prove the sixth login attempt within a minute "
            "gets 429 with a Retry-After header.",
            "depends_on": [limiter["id"]],
            "agents": agents(model),
        },
    )

    orchestrator = wait_for(
        lambda: next(
            iter(
                call(
                    base_url,
                    "GET",
                    f"/v1/sessions?goal={goal['id']}&seat=orchestrator",
                )["sessions"]
            ),
            None,
        ),
        "the orchestrator session",
    )
    call(
        base_url,
        "POST",
        f"/v1/goals/{goal['id']}/finalize",
        {},
        session=orchestrator["id"],
    )

    try:
        wait_for(
            lambda: call(base_url, "GET", f"/v1/tasks/{limiter['id']}")["status"]
            != "pending",
            "the first task to leave pending",
            timeout=10.0,
        )
    except TimeoutError:
        pass

    print(goal["id"])
    print(limiter["id"])


if __name__ == "__main__":
    main()
