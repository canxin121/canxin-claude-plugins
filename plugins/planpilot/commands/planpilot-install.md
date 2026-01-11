---
name: planpilot-install
description: Build and install the Planpilot CLI using cargo.
argument-hint: [cargo-install-flags]
allowed-tools: ["Bash(cargo:*)"]
---

# Planpilot Install

Install the Planpilot CLI from this plugin's source.

## Command

!`cargo install --path ${CLAUDE_PLUGIN_ROOT}`

## Notes

- If the command fails, explain the error and suggest installing Rust/Cargo.
- Remind the user that the binary is typically installed into `~/.cargo/bin` and should be on PATH.
