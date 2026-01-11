#!/usr/bin/env bash
set -euo pipefail

if ! command -v planpilot >/dev/null 2>&1; then
  echo '{"decision":"approve"}'
  exit 0
fi

set +e
planpilot hook stop
status=$?
set -e

if [ "$status" -eq 127 ]; then
  echo '{"decision":"approve"}'
  exit 0
fi

exit "$status"
