---
name: security-review
description: Find the vulnerabilities in a change or a component, each with its severity, its impact and its fix.
---

# Security review

Work out what an attacker controls, then follow it through the code.

## Steps

1. Name the assets, the trust boundaries and the untrusted input.
2. Follow each untrusted input to every place it is used.
3. Check the classes below against the code you read.
4. Confirm each finding against the real code path. Discard what cannot be
   reached.
5. Report each finding with its severity, its impact and its fix.

## What to look for

- Injection: SQL, shell, template, path traversal, deserialization.
- Authentication: who is allowed in, and how that is checked.
- Authorization: an object reference the caller does not own.
- Secrets: a key, a token or a password in code, in logs or in an error.
- Cryptography: a home-made scheme, a weak algorithm, a fixed nonce or salt.
- Input validation: a length, a type or a range nobody checks.
- Dependencies: a version with a known vulnerability.
- Output: data returned to a caller who is not allowed to see it.

## Rules

- Report the class of problem and the fix. Write no working exploit.
- Give every finding a reachable path. An unreachable one is a note, not a
  finding.
- Rate severity by impact and by how easily the path is reached.
- Report what you could not check.

## Done

Each finding names its path, its impact, its severity and its fix. The report
says what was in scope and what was not.
