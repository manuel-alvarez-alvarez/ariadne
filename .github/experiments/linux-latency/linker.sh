#!/usr/bin/env bash
# Stable driver path shared by cold, warm and replay; preserve every argument.
set -euo pipefail
exec python3 "$(dirname "$0")/linker.py" "$@"
