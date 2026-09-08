---
name: security-review
description: Find reachable vulnerabilities with severity, impact and fixes. Use when code, a data flow, or dependency needs security review.
---

# Security review

Find what an attacker controls. Follow each path through the code.

## Steps

1. Name the assets, trust boundaries and untrusted inputs.
   Done when every in-scope entry path crosses a named boundary.
2. Follow each untrusted input to every use. Record validation and ownership
   checks along its path.
   Done when each input reaches a terminal use or a trusted conversion.
3. Check every vulnerability class below against each reachable path.
   Done when every class has evidence or a recorded absence.
4. Confirm each candidate against the real code path. Remove unreachable
   candidates from the findings.
   Done when every finding has a reproducible path.
5. Report severity, impact and fix. Report each area you could not check.
   Done when every finding has all four parts.

## What to look for

- Injection: Check SQL, shell, templates, paths and deserialization.
- Authentication: Check who enters and how the code proves identity.
- Authorization: Check whether the caller owns each referenced object.
- Secrets: Check code, logs and errors for keys, tokens and passwords.
- Cryptography: Check algorithms, nonces, salts and custom schemes.
- Input validation: Check lengths, types and ranges.
- Dependencies: Check each changed version for known vulnerabilities.
- Output: Check whether the caller can receive each returned value.

## Rules

- Give every finding a reachable path.
- Record an unreachable candidate as a note.
- Rate severity from impact and path accessibility.
- Describe the vulnerability class and the fix.
- Never provide a working exploit.

## Do not tell yourself

- "The input is internal." -> Every trust boundary needs evidence.
- "The framework handles it." -> Confirm the protection on the real path.
- "The path looks unlikely." -> Reachability and impact determine severity.

## Done

Each finding names its path, impact, severity and fix. The report names the
reviewed scope, all unchecked areas and all unreachable notes.
