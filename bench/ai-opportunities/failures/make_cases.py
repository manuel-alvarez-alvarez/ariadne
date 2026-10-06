#!/usr/bin/env python3
"""Build the frozen, synthetic failure-diagnosis corpus."""
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent
TEMPLATES = [
 ("exhausted", "switch_model", "The provider returned usageLimitExceeded after the account used its monthly allocation.", {"codexErrorInfo": "usageLimitExceeded"}, "protocol"),
 ("exhausted", "switch_model", "Request denied: monthly credits are depleted; select another model or wait for renewal.", {}, "provider"),
 ("temporary", "retry", "The upstream service is temporarily unavailable after an overloaded worker response.", {}, "provider"),
 ("auth_config", "fix_configuration", "Unauthorized: invalid API key for the configured provider.", {}, "provider"),
 ("task_error", "task_action", "Command failed because tests/fixture.json is missing from this branch.", {}, "task"),
 ("insufficient", "abstain", "The agent reported an error without a message or error data.", {}, "protocol"),
 ("temporary", "retry", "Gateway timeout while contacting the agent backend; retry is safe.", {}, "provider"),
 ("auth_config", "fix_configuration", "The selected model is not configured for this account.", {}, "provider"),
 ("task_error", "task_action", "The compiler rejected the request because the manifest has invalid syntax.", {}, "task"),
 ("insufficient", "abstain", "The transcript says: quoted error 'rate limit' from a documentation example.", {}, "task"),
 ("task_error", "task_action", "This is not a rate limit; the repository quota report has 3 GB free.", {}, "task"),
 ("insufficient", "abstain", "Token quota: 8400 input tokens were counted for this successful prompt.", {}, "protocol"),
 ("exhausted", "switch_model", "The service says the rate allocation is exhausted until the billing period renews.", {}, "provider"),
 ("temporary", "retry", "Connection reset by peer during a request; no provider limit is reported.", {}, "provider"),
 ("auth_config", "fix_configuration", "Forbidden: this organization has no permission for the requested model.", {}, "provider"),
 ("task_error", "task_action", "The tool rejected argument path because it is outside the workspace.", {}, "task"),
 ("insufficient", "abstain", "Operation failed with code E_UNKNOWN.", {}, "protocol"),
 ("exhausted", "switch_model", "The ACP data marks the session failure category as limit.", {"_meta": {"jetbrains": {"air": {"sessionFailure": {"category": "limit"}}}}}, "protocol"),
 ("temporary", "retry", "Provider returned 503 overloaded; retry later.", {}, "provider"),
 ("auth_config", "fix_configuration", "Authentication expired and the refresh token is absent.", {}, "provider"),
 ("task_error", "task_action", "Git cannot continue because the working tree has unresolved conflicts.", {}, "task"),
 ("insufficient", "abstain", "The remote end closed the session.", {}, "protocol"),
 ("exhausted", "switch_model", "Usage limit reached for this model; reset at 00:00 UTC.", {}, "provider"),
 ("temporary", "retry", "The network DNS lookup failed once while resolving the provider.", {}, "provider"),
 ("task_error", "task_action", "A log line mentions quota, but the command failed because its JSON input is malformed.", {}, "task"),
]

def main() -> None:
    cases=[]
    for family, item in enumerate(TEMPLATES):
        label, recovery, message, data, provenance = item; split = "dev" if family < 16 else "eval"
        for session in range(5):
            cases.append({"id": f"failure-{family:02d}-{session:02d}", "split": split, "source_family": f"synthetic-{family:02d}", "session": f"synthetic-{family:02d}", "provenance": "synthetic", "message": message.replace(".", f" (session {session}).", 1), "data": data, "label": label, "recovery": recovery, "rationale": f"The explicit evidence supports {label}; the recovery category is {recovery}."})
    assert len(cases) == 125 and sum(c["split"] == "eval" for c in cases) == 45
    (ROOT / "cases.jsonl").write_text("\n".join(json.dumps(case) for case in cases) + "\n")

if __name__ == "__main__": main()
