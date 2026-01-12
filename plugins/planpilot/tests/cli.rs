use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
use serde_json::Value;
use tempfile::TempDir;
use url::Url;

fn bin_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_planpilot"))
}

fn run_cmd_with_env(
    cwd: Option<&Path>,
    session_id: Option<&str>,
    args: &[&str],
    input: Option<&str>,
) -> Output {
    let mut cmd = Command::new(bin_path());
    if let Some(cwd) = cwd {
        cmd.arg("--cwd").arg(cwd);
    }
    if let Some(session_id) = session_id {
        cmd.arg("--session-id").arg(session_id);
    }
    cmd.args(args);
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("spawn command");
    if let Some(input) = input {
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(input.as_bytes())
            .expect("write stdin");
    }
    child.wait_with_output().expect("wait output")
}

fn run_cmd(cwd: Option<&Path>, args: &[&str], input: Option<&str>) -> Output {
    run_cmd_with_env(cwd, Some("test-session"), args, input)
}

fn output_stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout utf8")
}

fn parse_plan_id(stdout: &str) -> i64 {
    let prefix = "Created plan ID: ";
    let rest = stdout.trim().strip_prefix(prefix).expect("plan output");
    let id_str = rest.split(':').next().expect("plan id");
    id_str.trim().parse().expect("plan id parse")
}

fn parse_step_id(stdout: &str) -> i64 {
    let prefix = "Created step ID: ";
    let rest = stdout.trim().strip_prefix(prefix).expect("step output");
    let id_str = rest.split_whitespace().next().expect("step id");
    id_str.parse().expect("step id parse")
}

fn parse_goal_id(stdout: &str) -> i64 {
    let prefix = "Created goal ID: ";
    let rest = stdout.trim().strip_prefix(prefix).expect("goal output");
    let id_str = rest.split_whitespace().next().expect("goal id");
    id_str.parse().expect("goal id parse")
}

fn create_plan(dir: &TempDir) -> i64 {
    let stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "add", "Plan", "Content"],
        None,
    ));
    parse_plan_id(&stdout)
}

fn add_step(dir: &TempDir, plan_id: i64, content: &str, executor: Option<&str>) -> i64 {
    let plan_id_str = plan_id.to_string();
    let mut args = vec!["step", "add", plan_id_str.as_str(), content];
    if let Some(executor) = executor {
        args.push("--executor");
        args.push(executor);
    }
    let stdout = output_stdout(run_cmd(Some(dir.path()), &args, None));
    parse_step_id(&stdout)
}

fn add_goal(dir: &TempDir, step_id: i64, content: &str) -> i64 {
    let stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "add", &step_id.to_string(), content],
        None,
    ));
    parse_goal_id(&stdout)
}

fn activate_plan(dir: &TempDir, plan_id: i64) {
    output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "activate", &plan_id.to_string()],
        None,
    ));
}

fn plan_md_path(dir: &TempDir, plan_id: i64) -> PathBuf {
    dir.path()
        .join(".claude")
        .join(".planpilot")
        .join("plans")
        .join(format!("plan_{plan_id}.md"))
}

#[test]
fn hook_pretooluse_injects_flags() {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "planpilot step show-next"},
        "session_id": "hook-session",
        "cwd": "/tmp/project",
        "permission_mode": "allow"
    });
    let output = run_cmd_with_env(
        None,
        None,
        &["hook", "pretooluse"],
        Some(&payload.to_string()),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let value: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(value["hookSpecificOutput"]["permissionDecision"], "allow");
    let command = value["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .expect("command");
    assert!(command.starts_with("planpilot --cwd /tmp/project --session-id hook-session"));
}

#[test]
fn hook_pretooluse_injects_flags_after_cd_chain() {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "cd /tmp&&planpilot step show-next"},
        "session_id": "hook-session",
        "cwd": "/tmp/project",
        "permission_mode": "allow"
    });
    let output = run_cmd_with_env(
        None,
        None,
        &["hook", "pretooluse"],
        Some(&payload.to_string()),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let value: Value = serde_json::from_str(&stdout).expect("json output");
    let command = value["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .expect("command");
    assert!(
        command.starts_with("cd /tmp&&planpilot --cwd /tmp/project --session-id hook-session"),
        "command: {command}"
    );
}

#[test]
fn hook_pretooluse_injects_flags_after_pipe() {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "echo hi | planpilot step show-next"},
        "session_id": "hook-session",
        "cwd": "/tmp/project",
        "permission_mode": "allow"
    });
    let output = run_cmd_with_env(
        None,
        None,
        &["hook", "pretooluse"],
        Some(&payload.to_string()),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let value: Value = serde_json::from_str(&stdout).expect("json output");
    let command = value["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .expect("command");
    assert!(
        command.contains("| planpilot --cwd /tmp/project --session-id hook-session step show-next"),
        "command: {command}"
    );
}

#[test]
fn hook_pretooluse_ignores_non_matching_command() {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "planpilot"},
        "session_id": "hook-session",
        "cwd": "/tmp/project",
        "permission_mode": "allow"
    });
    let output = run_cmd_with_env(
        None,
        None,
        &["hook", "pretooluse"],
        Some(&payload.to_string()),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.trim().is_empty());
}

#[test]
fn hook_pretooluse_ignores_quoted_planpilot() {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "echo \"planpilot step show-next\""},
        "session_id": "hook-session",
        "cwd": "/tmp/project",
        "permission_mode": "allow"
    });
    let output = run_cmd_with_env(
        None,
        None,
        &["hook", "pretooluse"],
        Some(&payload.to_string()),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    assert!(stdout.trim().is_empty());
}

#[test]
fn hook_pretooluse_quotes_cwd_and_session() {
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "planpilot step show-next"},
        "session_id": "session with space",
        "cwd": "/tmp/with space",
        "permission_mode": "allow"
    });
    let output = run_cmd_with_env(
        None,
        None,
        &["hook", "pretooluse"],
        Some(&payload.to_string()),
    );
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let value: Value = serde_json::from_str(&stdout).expect("json output");
    let command = value["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .expect("command");
    assert!(
        command.contains("--cwd '/tmp/with space' --session-id 'session with space'"),
        "command: {command}"
    );
}

#[test]
fn hook_stop_blocks_for_ai_step() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    add_step(&dir, plan_id, "Step 1", Some("ai"));
    activate_plan(&dir, plan_id);

    let payload = serde_json::json!({
        "session_id": "test-session",
        "cwd": dir.path().to_string_lossy()
    });
    let output = run_cmd_with_env(None, None, &["hook", "stop"], Some(&payload.to_string()));
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let value: Value = serde_json::from_str(&stdout).expect("json output");
    assert_eq!(value["decision"], "block");
    let reason = value["reason"].as_str().expect("reason");
    assert!(reason.starts_with("Planpilot (auto):"));
    assert!(reason.contains("Step ID:"));
    assert!(reason.contains("Executor: ai"));
}

#[test]
fn list_count_only_outputs_total() {
    let dir = TempDir::new().expect("temp dir");
    let plan_stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "add", "Plan", "Content"],
        None,
    ));
    let plan_id = parse_plan_id(&plan_stdout);

    let step_stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["step", "add", &plan_id.to_string(), "Step"],
        None,
    ));
    let step_id = parse_step_id(&step_stdout);

    output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "add", &step_id.to_string(), "Goal 1", "Goal 2"],
        None,
    ));

    let step_count = output_stdout(run_cmd(
        Some(dir.path()),
        &["step", "list", &plan_id.to_string(), "--count"],
        None,
    ));
    assert_eq!(step_count.trim(), "Total: 1");
    assert!(!step_count.contains("ID"));

    let goal_count = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "list", &step_id.to_string(), "--count"],
        None,
    ));
    assert_eq!(goal_count.trim(), "Total: 2");
    assert!(!goal_count.contains("ID"));
}

#[test]
fn plan_md_created_and_updated() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    activate_plan(&dir, plan_id);

    let md_path = plan_md_path(&dir, plan_id);
    assert!(md_path.exists());
    let contents = std::fs::read_to_string(&md_path).expect("read plan.md");
    assert!(contents.contains("## Plan: Plan\n"));
    assert!(contents.contains("**Active:** `true`"));

    output_stdout(run_cmd(
        Some(dir.path()),
        &[
            "plan",
            "update",
            &plan_id.to_string(),
            "--title",
            "Plan Updated",
        ],
        None,
    ));

    let contents = std::fs::read_to_string(&md_path).expect("read plan.md");
    assert!(contents.contains("## Plan: Plan Updated\n"));
    assert!(!contents.contains("## Plan: Plan\n"));
}

#[test]
fn plan_md_marked_inactive_on_deactivate() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    activate_plan(&dir, plan_id);

    let md_path = plan_md_path(&dir, plan_id);
    assert!(md_path.exists());

    output_stdout(run_cmd(Some(dir.path()), &["plan", "deactivate"], None));

    let contents = std::fs::read_to_string(&md_path).expect("read plan.md");
    assert!(contents.contains("**Active:** `false`"));
}

#[test]
fn plan_add_tree_creates_steps_and_goals() {
    let dir = TempDir::new().expect("temp dir");
    let stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &[
            "plan",
            "add-tree",
            "Tree Plan",
            "Plan content",
            "--step",
            "Step A",
            "--executor",
            "ai",
            "--goal",
            "Goal A1",
            "--goal",
            "Goal A2",
            "--step",
            "Step B",
            "--executor",
            "human",
        ],
        None,
    ));
    let plan_id = parse_plan_id(&stdout);

    let detail = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "show", &plan_id.to_string()],
        None,
    ));
    assert!(detail.contains("Step A"));
    assert!(detail.contains("Goal A1"));
    assert!(detail.contains("Goal A2"));
    assert!(detail.contains("Step B"));
}

#[test]
fn plan_add_tree_rejects_json_step_spec() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(
        Some(dir.path()),
        &[
            "plan",
            "add-tree",
            "Plan",
            "Content",
            "--step",
            r#"{"content":"Step"}"#,
        ],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no longer accepts JSON"));
}

#[test]
fn plan_add_tree_rejects_empty_step_content() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(
        Some(dir.path()),
        &["plan", "add-tree", "Plan", "Content", "--step", "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("plan add-tree --step cannot be empty"));
}

#[test]
fn plan_add_tree_rejects_empty_goal_content() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(
        Some(dir.path()),
        &[
            "plan",
            "add-tree",
            "Plan",
            "Content",
            "--step",
            "Step",
            "--goal",
            "   ",
        ],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("goal content cannot be empty"));
}

#[test]
fn step_add_tree_creates_goals() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &[
            "step",
            "add-tree",
            &plan_id.to_string(),
            "Step with goals",
            "--executor",
            "ai",
            "--goal",
            "G1",
            "--goal",
            "G2",
        ],
        None,
    ));
    let step_id = parse_step_id(&stdout);

    let goals = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "list", &step_id.to_string(), "--all"],
        None,
    ));
    assert!(goals.contains("G1"));
    assert!(goals.contains("G2"));
}

#[test]
fn step_comment_rejects_empty_comment() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", Some("ai"));
    let output = run_cmd(
        Some(dir.path()),
        &["step", "comment", &step_id.to_string(), "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("comment cannot be empty"));
}

#[test]
fn goal_done_multiple_ids_marks_all_done() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", Some("ai"));
    let g1 = add_goal(&dir, step_id, "G1");
    let g2 = add_goal(&dir, step_id, "G2");

    let output = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "done", &g1.to_string(), &g2.to_string()],
        None,
    ));
    assert!(output.contains("Goals marked done"));

    let goals = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "list", &step_id.to_string(), "--all"],
        None,
    ));
    assert!(goals.contains("done"));
}

#[test]
fn goal_comment_rejects_empty_comment() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", Some("ai"));
    let goal_id = add_goal(&dir, step_id, "Goal 1");
    let output = run_cmd(
        Some(dir.path()),
        &["goal", "comment", &goal_id.to_string(), "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("comment cannot be empty"));
}

#[test]
fn step_done_all_goals_marks_goals_and_step() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", Some("ai"));
    let _g1 = add_goal(&dir, step_id, "G1");
    let _g2 = add_goal(&dir, step_id, "G2");

    let output = output_stdout(run_cmd(
        Some(dir.path()),
        &["step", "done", &step_id.to_string(), "--all-goals"],
        None,
    ));
    assert!(output.contains("Step ID"));

    let goals = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "list", &step_id.to_string(), "--all"],
        None,
    ));
    assert!(goals.contains("done"));
}

#[test]
fn step_show_next_no_active_plan() {
    let dir = TempDir::new().expect("temp dir");
    let output = output_stdout(run_cmd(Some(dir.path()), &["step", "show-next"], None));
    assert_eq!(output.trim(), "No active plan.");
}

#[test]
fn step_show_next_displays_detail() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let _step_id = add_step(&dir, plan_id, "Step 1", Some("ai"));
    activate_plan(&dir, plan_id);

    let output = output_stdout(run_cmd(Some(dir.path()), &["step", "show-next"], None));
    assert!(output.contains("Step ID"));
    assert!(output.contains("Step 1"));
}

#[test]
fn step_done_with_next_ai_step_prompts_end_turn() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step1 = add_step(&dir, plan_id, "Step 1", Some("ai"));
    let _step2 = add_step(&dir, plan_id, "Step 2", Some("ai"));

    let output = output_stdout(run_cmd(
        Some(dir.path()),
        &["step", "done", &step1.to_string()],
        None,
    ));
    assert!(output.contains("Next step is assigned to ai"));
}

#[test]
fn goal_done_with_next_human_step_prints_detail() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step1 = add_step(&dir, plan_id, "Step 1", Some("ai"));
    let _step2 = add_step(&dir, plan_id, "Step 2", Some("human"));
    let goal_id = add_goal(&dir, step1, "Goal 1");

    let output = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "done", &goal_id.to_string()],
        None,
    ));
    assert!(output.contains("Next step requires human action:"));
    assert!(output.contains("Step ID"));
    assert!(output.contains("Step 2"));
    assert!(output.contains(
        "Tell the user to complete the above step and goals. Confirm each goal when done, then end this turn."
    ));
}

#[test]
fn plan_done_prompts_summary_and_end_turn() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);

    let output = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "done", &plan_id.to_string()],
        None,
    ));
    assert!(output.contains("Plan ID:"));
    assert!(output.contains("Summarize the completed results to the user, then end this turn."));
}

#[test]
fn plan_md_updates_on_step_add() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    activate_plan(&dir, plan_id);

    output_stdout(run_cmd(
        Some(dir.path()),
        &["step", "add", &plan_id.to_string(), "First step"],
        None,
    ));

    let contents = std::fs::read_to_string(plan_md_path(&dir, plan_id)).expect("read plan.md");
    assert!(contents.contains("First step"));
}

#[test]
fn plan_export_writes_markdown() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", Some("ai"));
    add_goal(&dir, step_id, "Goal 1");
    activate_plan(&dir, plan_id);

    let export_path = dir.path().join("export.md");
    output_stdout(run_cmd(
        Some(dir.path()),
        &[
            "plan",
            "export",
            &plan_id.to_string(),
            export_path.to_str().expect("export path"),
        ],
        None,
    ));

    let contents = std::fs::read_to_string(&export_path).expect("read export.md");
    assert!(contents.contains("## Plan: Plan"));
    assert!(contents.contains("Step 1"));
    assert!(contents.contains("Goal 1"));
    assert!(contents.contains("**Active:** `true`"));
}

#[test]
fn plan_list_filters_status() {
    let dir = TempDir::new().expect("temp dir");
    let todo_stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "add", "Todo Plan", "Content"],
        None,
    ));
    let _todo_id = parse_plan_id(&todo_stdout);

    let done_stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "add", "Done Plan", "Content"],
        None,
    ));
    let done_id = parse_plan_id(&done_stdout);
    output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "done", &done_id.to_string()],
        None,
    ));

    let stdout = output_stdout(run_cmd(Some(dir.path()), &["plan", "list"], None));
    assert!(stdout.contains("Todo Plan"));
    assert!(!stdout.contains("Done Plan"));

    let stdout_done = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "list", "--status", "done"],
        None,
    ));
    assert!(!stdout_done.contains("Todo Plan"));
    assert!(stdout_done.contains("Done Plan"));

    let stdout_all = output_stdout(run_cmd(Some(dir.path()), &["plan", "list", "--all"], None));
    assert!(stdout_all.contains("Todo Plan"));
    assert!(stdout_all.contains("Done Plan"));
}

#[test]
fn plan_auto_done_prompts_summary_and_end_turn() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", Some("ai"));

    let output = output_stdout(run_cmd(
        Some(dir.path()),
        &["step", "done", &step_id.to_string()],
        None,
    ));
    assert!(output.contains("Plan ID:"));
    assert!(output.contains("Summarize the completed results to the user, then end this turn."));
}

#[test]
fn step_add_rejects_empty_content() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);

    let output = run_cmd(
        Some(dir.path()),
        &["step", "add", &plan_id.to_string(), "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("step content cannot be empty"));
}

#[test]
fn step_update_rejects_empty_content() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", None);
    let output = run_cmd(
        Some(dir.path()),
        &["step", "update", &step_id.to_string(), "--content", "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("step content cannot be empty"));
}

#[test]
fn step_list_reports_missing_plan() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(Some(dir.path()), &["step", "list", "9999"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.trim(), "Error: Not found: plan id 9999");
}

#[test]
fn step_list_filters_status_and_executor() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let _alpha_id = add_step(&dir, plan_id, "Alpha", Some("ai"));
    let bravo_id = add_step(&dir, plan_id, "Bravo", Some("human"));
    let _charlie_id = add_step(&dir, plan_id, "Charlie", Some("human"));

    output_stdout(run_cmd(
        Some(dir.path()),
        &["step", "done", &bravo_id.to_string()],
        None,
    ));

    let stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &[
            "step",
            "list",
            &plan_id.to_string(),
            "--status",
            "todo",
            "--executor",
            "human",
        ],
        None,
    ));
    assert!(stdout.contains("Charlie"));
    assert!(!stdout.contains("Alpha"));
    assert!(!stdout.contains("Bravo"));
}

#[test]
fn goal_list_reports_missing_step() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(Some(dir.path()), &["goal", "list", "9999"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.trim(), "Error: Not found: step id 9999");
}

#[test]
fn plan_activate_rejects_done_plan() {
    let dir = TempDir::new().expect("temp dir");
    let plan_stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "add", "Plan", "Content"],
        None,
    ));
    let plan_id = parse_plan_id(&plan_stdout);

    output_stdout(run_cmd(
        Some(dir.path()),
        &["plan", "done", &plan_id.to_string()],
        None,
    ));

    let output = run_cmd(
        Some(dir.path()),
        &["plan", "activate", &plan_id.to_string()],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("cannot activate plan; plan is done"));
}

#[test]
fn plan_activate_reports_missing_plan() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(Some(dir.path()), &["plan", "activate", "9999"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.trim(), "Error: Not found: plan id 9999");
}

#[test]
fn step_remove_reports_missing_ids() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(Some(dir.path()), &["step", "remove", "9999"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.trim(),
        "Error: Not found: step id(s) not found: 9999"
    );
}

#[test]
fn goal_remove_reports_missing_ids() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(Some(dir.path()), &["goal", "remove", "9999"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.trim(),
        "Error: Not found: goal id(s) not found: 9999"
    );
}

#[test]
fn plan_add_rejects_empty_content() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(Some(dir.path()), &["plan", "add", "Plan", "   "], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("plan content cannot be empty"));
}

#[test]
fn plan_add_rejects_empty_title() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd(Some(dir.path()), &["plan", "add", "   ", "Content"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("plan title cannot be empty"));
}

#[test]
fn plan_update_rejects_empty_content() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let output = run_cmd(
        Some(dir.path()),
        &["plan", "update", &plan_id.to_string(), "--content", "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("plan content cannot be empty"));
}

#[test]
fn plan_update_rejects_empty_title() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let output = run_cmd(
        Some(dir.path()),
        &["plan", "update", &plan_id.to_string(), "--title", "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("plan title cannot be empty"));
}

#[test]
fn goal_add_rejects_empty_content() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", None);
    let output = run_cmd(
        Some(dir.path()),
        &["goal", "add", &step_id.to_string(), "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("goal content cannot be empty"));
}

#[test]
fn goal_update_rejects_empty_content() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", None);
    let goal_id = add_goal(&dir, step_id, "Goal 1");
    let output = run_cmd(
        Some(dir.path()),
        &["goal", "update", &goal_id.to_string(), "--content", "   "],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("goal content cannot be empty"));
}

#[test]
fn goal_list_filters_status_and_pagination() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", None);
    let _goal1 = add_goal(&dir, step_id, "G1");
    let goal2 = add_goal(&dir, step_id, "G2");
    let _goal3 = add_goal(&dir, step_id, "G3");

    output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "done", &goal2.to_string()],
        None,
    ));

    let stdout_done = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "list", &step_id.to_string(), "--status", "done"],
        None,
    ));
    assert!(stdout_done.contains("G2"));
    assert!(!stdout_done.contains("G1"));
    assert!(!stdout_done.contains("G3"));

    let stdout_page = output_stdout(run_cmd(
        Some(dir.path()),
        &[
            "goal",
            "list",
            &step_id.to_string(),
            "--all",
            "--limit",
            "1",
            "--offset",
            "1",
        ],
        None,
    ));
    assert!(stdout_page.contains("G2"));
    assert!(!stdout_page.contains("G1"));
    assert!(!stdout_page.contains("G3"));
}

#[test]
fn goal_show_outputs_detail() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", None);
    let goal_id = add_goal(&dir, step_id, "Goal 1");

    let stdout = output_stdout(run_cmd(
        Some(dir.path()),
        &["goal", "show", &goal_id.to_string()],
        None,
    ));

    assert!(stdout.contains(&format!("Goal ID: {}", goal_id)));
    assert!(stdout.contains(&format!("Step ID: {}", step_id)));
    assert!(stdout.contains(&format!("Plan ID: {}", plan_id)));
    assert!(stdout.contains("Status: todo"));
    assert!(stdout.contains("Content: Goal 1"));
    assert!(stdout.contains("Step Content: Step 1"));
}

#[test]
fn step_add_rejects_zero_position() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let output = run_cmd(
        Some(dir.path()),
        &["step", "add", &plan_id.to_string(), "Step", "--at", "0"],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("position starts at 1"));
}

#[test]
fn step_move_rejects_zero_position() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    let step_id = add_step(&dir, plan_id, "Step 1", None);
    let output = run_cmd(
        Some(dir.path()),
        &["step", "move", &step_id.to_string(), "--to", "0"],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("position starts at 1"));
}

#[test]
fn plan_show_active_when_none() {
    let dir = TempDir::new().expect("temp dir");
    let stdout = output_stdout(run_cmd(Some(dir.path()), &["plan", "show-active"], None));
    assert_eq!(stdout.trim(), "No active plan.");
}

#[test]
fn plan_deactivate_clears_active() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    activate_plan(&dir, plan_id);

    output_stdout(run_cmd(Some(dir.path()), &["plan", "deactivate"], None));

    let stdout = output_stdout(run_cmd(Some(dir.path()), &["plan", "show-active"], None));
    assert_eq!(stdout.trim(), "No active plan.");
}

#[test]
fn missing_cwd_flag_errors() {
    let output = run_cmd_with_env(None, Some("test-session"), &["plan", "list"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--cwd"));
}

#[test]
fn empty_cwd_flag_errors() {
    let output = run_cmd_with_env(
        Some(Path::new("   ")),
        Some("test-session"),
        &["plan", "list"],
        None,
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--cwd is empty"));
}

#[test]
fn missing_session_id_flag_errors() {
    let dir = TempDir::new().expect("temp dir");
    let output = run_cmd_with_env(Some(dir.path()), None, &["plan", "list"], None);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--session-id"));
}

#[tokio::test]
async fn show_active_clears_orphaned_active_plan() {
    let dir = TempDir::new().expect("temp dir");
    let plan_id = create_plan(&dir);
    activate_plan(&dir, plan_id);

    let db_path = dir
        .path()
        .join(".claude")
        .join(".planpilot")
        .join("planpilot.db");
    let mut url = Url::from_file_path(&db_path).expect("db path");
    url.set_query(Some("mode=rwc"));
    let sqlite_url = url.as_str().replacen("file://", "sqlite://", 1);
    let db = Database::connect(&sqlite_url).await.expect("connect db");
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA foreign_keys = OFF;".to_string(),
    ))
    .await
    .expect("disable fk");
    let invalid_id = plan_id + 9999;
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        format!("UPDATE active_plan SET plan_id = {invalid_id};"),
    ))
    .await
    .expect("update active plan");

    let stdout = output_stdout(run_cmd(Some(dir.path()), &["plan", "show-active"], None));
    assert!(stdout.contains(&format!("Active plan ID: {} not found.", invalid_id)));

    let stdout = output_stdout(run_cmd(Some(dir.path()), &["plan", "show-active"], None));
    assert_eq!(stdout.trim(), "No active plan.");
}
