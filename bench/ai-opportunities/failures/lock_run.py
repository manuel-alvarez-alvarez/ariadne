#!/usr/bin/env python3
"""Run one foreground command while holding the shared experiment lock."""
import fcntl
import os
import subprocess
import sys

LOCK = "/tmp/ariadne-ai-01m48gt1v7v045wtc1kaw0fnas.lock"

with open(LOCK, "a+", encoding="utf-8") as handle:
    fcntl.flock(handle, fcntl.LOCK_EX)
    raise SystemExit(subprocess.run(sys.argv[1:], check=False).returncode)
