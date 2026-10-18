"""Acquires the shared advisory lock, then runs `worker/kev_infer.py` inside the interpreter
that already has `kev` installed, for exactly as long as the model process is alive. This
module needs nothing `kev` provides itself; it only shells out to another interpreter, so it
never imports another experiment's code.

Per the orchestrator's provisioning note: the interpreter at
`~/.ariadne/ai-permissions/venv/bin/python3` already carries `kev` pinned to the required
revision (`f1535963cea021439370c23127bc970b6788e730`) and is reused read-only, as the
"production installation [to keep] intact" - this script never installs into it or writes to
it. The checkpoint weights are cached separately under `~/.ariadne/ai-opportunities/hf`, the
shared experimental `HF_HOME` three sibling experiments provision and read from, rather than
the production `ai-permissions` cache.

The lock is an OS advisory lock (`fcntl.flock`) on an open file descriptor: it releases the
moment this process exits, by any means, because the kernel drops the lock with the file
descriptor table, which is what "releases when the owning process exits" (the task's lock rule)
requires without this script having to catch every exit path itself. Provisioning (the revision
check) and inference both run under it, since both touch the shared, cross-experiment state at
those paths.
"""
from __future__ import annotations

import contextlib
import fcntl
import json
import os
import subprocess
import time

LOCK_PATH = "/tmp/ariadne-ai-01m48gt1v7v045wtc1kaw0fnas.lock"
#: The interpreter the orchestrator confirmed already carries `kev` at the required revision -
#: reused read-only, never reinstalled into.
KEV_PYTHON = os.path.expanduser("~/.ariadne/ai-permissions/venv/bin/python3")
#: The shared experimental cache three sibling experiments provision into and read from -
#: never the production `ai-permissions/hf`.
HF_HOME = os.path.expanduser("~/.ariadne/ai-opportunities/hf")
REQUIRED_KEV_REVISION = "f1535963cea021439370c23127bc970b6788e730"
REQUIRED_RUN = "jaredpalmer/kev-4b@139fdd94f1b6a6ad80cc15e08fcb99cac885a101"
HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WORKER = os.path.join(HERE, "worker", "kev_infer.py")


@contextlib.contextmanager
def _advisory_lock():
    os.makedirs(os.path.dirname(LOCK_PATH) or ".", exist_ok=True)
    lock_fd = os.open(LOCK_PATH, os.O_CREAT | os.O_RDWR, 0o644)
    try:
        waited = False
        while True:
            try:
                fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if not waited:
                    print(f"[kev_runner] lock busy at {LOCK_PATH}, waiting for another experiment to finish...")
                    waited = True
                time.sleep(2.0)
        yield
    finally:
        fcntl.flock(lock_fd, fcntl.LOCK_UN)
        os.close(lock_fd)


def check_kev_revision(kev_python: str = KEV_PYTHON) -> str:
    """Reads the installed `kev` package's git revision from its `direct_url.json` (pip's own
    record of what it installed from), without importing it in this interpreter. Raises if the
    interpreter is missing `kev` or carries a different revision than the one required -
    an infrastructure blocker, not a model-quality result."""
    if not os.path.exists(kev_python):
        raise RuntimeError(
            f"no interpreter at {kev_python}; the orchestrator's provisioning note names this "
            "path as the one that already carries `kev` - it is missing or was moved"
        )
    probe = (
        "import importlib.metadata as m, json, sys\n"
        "try:\n"
        "    d = m.distribution('kev')\n"
        "except m.PackageNotFoundError:\n"
        "    print(json.dumps(None)); sys.exit(0)\n"
        "url_json = d.read_text('direct_url.json')\n"
        "print(url_json or json.dumps(None))\n"
    )
    result = subprocess.run([kev_python, "-c", probe], capture_output=True, text=True, timeout=30)
    if result.returncode != 0:
        raise RuntimeError(f"could not query kev's installed revision: {result.stderr.strip()}")
    info = json.loads(result.stdout.strip() or "null")
    if not info:
        raise RuntimeError(f"{kev_python} has no `kev` package installed")
    revision = (info.get("vcs_info") or {}).get("commit_id")
    if revision != REQUIRED_KEV_REVISION:
        raise RuntimeError(
            f"{kev_python}'s kev is at revision {revision!r}, required {REQUIRED_KEV_REVISION!r}"
        )
    return revision


def run_kev_batch(
    prompt_path: str,
    input_path: str,
    output_path: str,
    stats_path: str,
    policy_name: str,
    kev_python: str = KEV_PYTHON,
    hf_home: str = HF_HOME,
    timeout_s: float = 1800.0,
) -> None:
    if not os.path.exists(kev_python):
        raise RuntimeError(
            f"no kev interpreter at {kev_python}; the orchestrator's provisioning note is the "
            "source of truth for where to find or build one - see this module's docstring"
        )

    with _advisory_lock():
        check_kev_revision(kev_python)
        os.makedirs(hf_home, exist_ok=True)

        env = dict(os.environ)
        env["HF_HOME"] = hf_home
        cmd = [
            kev_python,
            WORKER,
            "--prompt",
            prompt_path,
            "--input",
            input_path,
            "--output",
            output_path,
            "--stats",
            stats_path,
            "--policy-name",
            policy_name,
        ]
        result = subprocess.run(cmd, env=env, timeout=timeout_s)
        if result.returncode != 0:
            raise RuntimeError(f"kev_infer.py exited {result.returncode}")


if __name__ == "__main__":
    import argparse

    ap = argparse.ArgumentParser()
    ap.add_argument("--prompt", required=True)
    ap.add_argument("--input", required=True)
    ap.add_argument("--output", required=True)
    ap.add_argument("--stats", required=True)
    ap.add_argument("--policy-name", required=True)
    args = ap.parse_args()
    run_kev_batch(args.prompt, args.input, args.output, args.stats, args.policy_name)
