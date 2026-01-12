use serde::Deserialize;
use serde_json::json;
use shell_escape::escape;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Deserialize)]
struct HookInput {
    session_id: Option<String>,
    cwd: Option<String>,
    permission_mode: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<ToolInput>,
}

#[derive(Debug, Deserialize)]
struct ToolInput {
    command: Option<String>,
}

pub fn run_stop_hook() {
    let payload = match read_stdin() {
        Ok(payload) => payload,
        Err(_) => {
            print_approve();
            return;
        }
    };

    if payload.trim().is_empty() {
        print_approve();
        return;
    }

    let input: HookInput = match serde_json::from_str(&payload) {
        Ok(input) => input,
        Err(_) => {
            print_approve();
            return;
        }
    };

    let session_id = input.session_id.unwrap_or_default();
    let cwd = input.cwd.unwrap_or_default();
    if session_id.trim().is_empty() || cwd.trim().is_empty() {
        print_approve();
        return;
    }

    let output = match planpilot_show_next(&cwd, &session_id) {
        Some(output) => output,
        None => {
            print_approve();
            return;
        }
    };

    let stripped = output.trim_end();
    if stripped.is_empty() {
        print_approve();
        return;
    }

    if stripped.starts_with("No active plan.") || stripped.starts_with("No pending step.") {
        print_approve();
        return;
    }

    let executor = extract_executor(stripped).unwrap_or_default();
    if executor != "ai" {
        print_approve();
        return;
    }

    let message = format!(
        "Planpilot (auto):\nBefore acting, think through the next step and its goals. Record implementation details using Planpilot comments (plan/step/goal --comment or comment commands). Continue with the next step (executor: ai). Do not ask for confirmation; proceed and report results.\n\n{stripped}"
    );
    print_block(&message);
}

pub fn run_pretooluse_hook() {
    let payload = match read_stdin() {
        Ok(payload) => payload,
        Err(_) => {
            return;
        }
    };

    if payload.trim().is_empty() {
        return;
    }

    let input: HookInput = match serde_json::from_str(&payload) {
        Ok(input) => input,
        Err(_) => {
            return;
        }
    };

    if input.tool_name.as_deref() != Some("Bash") {
        return;
    }

    let command = match input.tool_input.and_then(|tool| tool.command) {
        Some(command) if !command.trim().is_empty() => command,
        _ => {
            return;
        }
    };

    if !command_matches(&command) {
        return;
    }

    let session_id = input.session_id.unwrap_or_default();
    let cwd = input.cwd.unwrap_or_default();
    if session_id.trim().is_empty() || cwd.trim().is_empty() {
        return;
    }

    let permission_mode = input.permission_mode.unwrap_or_else(|| "allow".to_string());
    let permission_decision = if permission_mode == "ask" {
        "ask"
    } else {
        "allow"
    };

    let updated_command = inject_flags(&command, &cwd, &session_id);

    let output = json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": permission_decision,
            "updatedInput": {"command": updated_command},
        }
    });
    print!("{}", output.to_string());
}

fn read_stdin() -> io::Result<String> {
    let mut buffer = String::new();
    io::stdin().read_to_string(&mut buffer)?;
    Ok(buffer)
}

fn planpilot_show_next(cwd: &str, session_id: &str) -> Option<String> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("planpilot"));
    let output = Command::new(exe)
        .arg("--cwd")
        .arg(cwd)
        .arg("--session-id")
        .arg(session_id)
        .arg("step")
        .arg("show-next")
        .output();

    let output = match output {
        Ok(output) => output,
        Err(_) => {
            return None;
        }
    };

    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(combined)
}

fn extract_executor(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.strip_prefix("Executor: ").map(str::trim))
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn command_matches(command: &str) -> bool {
    find_planpilot_insertion(command).is_some()
}

fn inject_flags(command: &str, cwd: &str, session_id: &str) -> String {
    if command.contains("--cwd") || command.contains("--session-id") {
        return command.to_string();
    }

    let insert_at = match find_planpilot_insertion(command) {
        Some(position) => position,
        None => return command.to_string(),
    };

    let mut updated = String::new();
    updated.push_str(&command[..insert_at]);
    updated.push_str(" --cwd ");
    updated.push_str(&escape(cwd.into()));
    updated.push_str(" --session-id ");
    updated.push_str(&escape(session_id.into()));
    updated.push_str(&command[insert_at..]);
    updated
}

fn find_planpilot_insertion(command: &str) -> Option<usize> {
    let bytes = command.as_bytes();
    let word = b"planpilot";
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    let mut escape_next = false;
    let mut at_command_start = true;

    while i < bytes.len() {
        let b = bytes[i];

        if escape_next {
            escape_next = false;
            at_command_start = false;
            i += 1;
            continue;
        }

        if in_single {
            if b == b'\'' {
                in_single = false;
            }
            i += 1;
            continue;
        }

        if in_double {
            match b {
                b'"' => {
                    in_double = false;
                    i += 1;
                    continue;
                }
                b'\\' => {
                    escape_next = true;
                    i += 1;
                    continue;
                }
                _ => {
                    i += 1;
                    continue;
                }
            }
        }

        match b {
            b'\\' => {
                escape_next = true;
                i += 1;
                continue;
            }
            b'\'' => {
                in_single = true;
                i += 1;
                continue;
            }
            b'"' => {
                in_double = true;
                i += 1;
                continue;
            }
            _ => {}
        }

        if b.is_ascii_whitespace() {
            if matches!(b, b'\n' | b'\r') {
                at_command_start = true;
            }
            i += 1;
            continue;
        }

        if is_separator(b) {
            at_command_start = true;
            i += 1;
            continue;
        }

        if at_command_start && bytes[i..].starts_with(word) {
            let after = i + word.len();
            if after < bytes.len() && bytes[after].is_ascii_whitespace() {
                let next_non_ws = bytes[after..]
                    .iter()
                    .position(|byte| !byte.is_ascii_whitespace());
                if let Some(offset) = next_non_ws {
                    let next_char = bytes[after + offset];
                    if !is_separator(next_char) {
                        return Some(after);
                    }
                }
            }
        }

        at_command_start = false;
        i += 1;
    }

    None
}

fn is_separator(byte: u8) -> bool {
    matches!(byte, b'&' | b'|' | b';')
}

fn print_approve() {
    print!("{}", json!({"decision": "approve"}).to_string());
}

fn print_block(message: &str) {
    print!(
        "{}",
        json!({"decision": "block", "reason": message}).to_string()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_matches_requires_planpilot_space_prefix() {
        assert!(command_matches("planpilot step show-next"));
        assert!(command_matches("  planpilot step show-next"));
        assert!(command_matches("cd /tmp && planpilot step show-next"));
        assert!(command_matches("cd /tmp&&planpilot step show-next"));
        assert!(command_matches("planpilot\tstep show-next"));
        assert!(command_matches("echo hi | planpilot step show-next"));
        assert!(command_matches("pwd;planpilot step show-next"));
        assert!(!command_matches("planpilot"));
        assert!(!command_matches("planpilot && echo hi"));
        assert!(!command_matches("planpilot;echo hi"));
        assert!(!command_matches("planpilot.sh step"));
        assert!(!command_matches("echo planpilot step"));
        assert!(!command_matches("echo 'planpilot step show-next'"));
        assert!(!command_matches("echo \"planpilot step show-next\""));
    }

    #[test]
    fn inject_flags_preserves_leading_whitespace() {
        let updated = inject_flags("  planpilot step show-next", "/tmp", "abc");
        assert!(updated.starts_with("  planpilot --cwd /tmp --session-id abc"));
    }

    #[test]
    fn inject_flags_inserts_after_chained_planpilot() {
        let updated = inject_flags("cd /tmp&&planpilot step show-next", "/tmp", "abc");
        assert!(
            updated.starts_with("cd /tmp&&planpilot --cwd /tmp --session-id abc"),
            "command: {updated}"
        );
    }

    #[test]
    fn inject_flags_inserts_after_pipe() {
        let updated = inject_flags("echo hi | planpilot step show-next", "/tmp", "abc");
        assert!(
            updated.contains("| planpilot --cwd /tmp --session-id abc step show-next"),
            "command: {updated}"
        );
    }

    #[test]
    fn inject_flags_skips_quoted_planpilot() {
        let command = "echo 'planpilot step show-next'";
        let updated = inject_flags(command, "/tmp", "abc");
        assert_eq!(updated, command);
    }

    #[test]
    fn inject_flags_skips_when_flags_present() {
        let command = "planpilot --cwd /tmp --session-id abc step show-next";
        let updated = inject_flags(command, "/tmp", "abc");
        assert_eq!(updated, command);
    }

    #[test]
    fn inject_flags_escapes_parens_and_spaces() {
        let command = "planpilot plan add-tree \"Todolist backend (Rust/Axum/SeaORM)\" \"Build a plan\" --step \"{\\\"content\\\":\\\"Step A\\\"}\"";
        let updated = inject_flags(command, "/tmp", "abc");
        assert!(updated.contains("\"Todolist backend (Rust/Axum/SeaORM)\""));
        assert!(updated.contains("\"Build a plan\""));
        assert!(updated.contains("\"{\\\"content\\\":\\\"Step A\\\"}\""));
    }

    #[test]
    fn inject_flags_escapes_single_quotes() {
        let command = "planpilot plan add \"Bob's plan\" \"O'Reilly content\"";
        let updated = inject_flags(command, "/tmp", "abc");
        assert!(updated.contains("\"Bob's plan\""));
        assert!(updated.contains("\"O'Reilly content\""));
    }

    #[test]
    fn shell_escape_quotes_values() {
        assert_eq!(escape("simple".into()), "simple");
        assert_eq!(escape("has space".into()), "'has space'");
        assert_eq!(escape("has'quote".into()), "'has'\\''quote'");
    }
}
