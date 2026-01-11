# Planpilot

Planpilot is a Claude Code plugin that keeps multi-step work moving. When the AI finishes a reply before a plan is complete, the stop hook tells the AI to continue with the next unfinished step so the plan keeps progressing until it ends or needs human help. This repository is part of the canxin-claude-plugins collection and packages Planpilot as a standard plugin with commands and hooks.

## Features
- Stop-hook instructs the AI to continue with the next pending step.
- Plan/step/goal hierarchy with automatic status rollups.
- Local-first storage per workspace (SQLite + readable plan snapshots).
- Claude Code hook injects required context flags for planpilot CLI calls.

## Requirements
- Rust/Cargo (to build and install the CLI).

## Details
See [planpilot.md](commands/planpilot.md) for details.
