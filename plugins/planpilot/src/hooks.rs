use serde::Deserialize;
use serde_json::json;
use shell_escape::escape;
use shlex::Shlex;
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
    command.trim_start().starts_with("planpilot ")
}

fn inject_flags(command: &str, cwd: &str, session_id: &str) -> String {
    if command.contains("--cwd") || command.contains("--session-id") {
        return command.to_string();
    }

    let trimmed = command.trim_start();
    if trimmed.is_empty() {
        return command.to_string();
    }

    let leading_len = command.len() - trimmed.len();
    let mut lexer = Shlex::new(trimmed);
    let first = match lexer.next() {
        Some(token) => token,
        None => return command.to_string(),
    };
    if first != "planpilot" {
        return command.to_string();
    }

    let rest = lexer
        .collect::<Vec<String>>()
        .into_iter()
        .map(|token| escape(token.into()).to_string())
        .collect::<Vec<String>>()
        .join(" ");
    let mut updated = String::new();
    updated.push_str(&command[..leading_len]);
    updated.push_str(&first);
    updated.push_str(" --cwd ");
    updated.push_str(&escape(cwd.into()));
    updated.push_str(" --session-id ");
    updated.push_str(&escape(session_id.into()));
    if !rest.is_empty() {
        updated.push(' ');
        updated.push_str(&rest);
    }
    updated
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
        assert!(!command_matches("planpilot"));
        assert!(!command_matches("planpilot.sh step"));
        assert!(!command_matches("echo planpilot step"));
    }

    #[test]
    fn inject_flags_preserves_leading_whitespace() {
        let updated = inject_flags("  planpilot step show-next", "/tmp", "abc");
        assert!(updated.starts_with("  planpilot --cwd /tmp --session-id abc"));
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
        assert!(updated.contains("'Todolist backend (Rust/Axum/SeaORM)'"));
        assert!(updated.contains("'Build a plan'"));
        assert!(updated.contains("'{\"content\":\"Step A\"}'"));
    }

    #[test]
    fn inject_flags_escapes_single_quotes() {
        let command = "planpilot plan add \"Bob's plan\" \"O'Reilly content\"";
        let updated = inject_flags(command, "/tmp", "abc");
        assert!(updated.contains("'Bob'\\''s plan'"));
        assert!(updated.contains("'O'\\''Reilly content'"));
    }

    #[test]
    fn shell_escape_quotes_values() {
        assert_eq!(escape("simple".into()), "simple");
        assert_eq!(escape("has space".into()), "'has space'");
        assert_eq!(escape("has'quote".into()), "'has'\\''quote'");
    }
}
