#!/usr/bin/env bash
set -euo pipefail

payload="$(cat)"
if [ -z "$payload" ]; then
  exit 0
fi

if ! command -v jq >/dev/null 2>&1; then
  exit 0
fi

tool_name="$(jq -r '.tool_name // empty' <<<"$payload")"
if [ "$tool_name" != "Bash" ]; then
  exit 0
fi

command="$(jq -r '.tool_input.command // empty' <<<"$payload")"
trimmed="${command#"${command%%[![:space:]]*}"}"
case "$trimmed" in
  planpilot\ *) ;;
  *) exit 0 ;;
esac

if ! command -v planpilot >/dev/null 2>&1; then
  exit 0
fi

printf '%s' "$payload" | planpilot hook pretooluse
