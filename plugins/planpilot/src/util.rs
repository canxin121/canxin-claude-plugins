use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::entities::{goal, plan, step};
use crate::model::GoalStatus;

fn has_text(value: &Option<String>) -> bool {
    value
        .as_deref()
        .map(|text| !text.trim().is_empty())
        .unwrap_or(false)
}

pub fn format_datetime(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M").to_string()
}

pub fn format_step_detail(step: &step::Model, goals: &[goal::Model]) -> String {
    let mut output = String::new();
    output.push_str(&format!("Step ID: {}\n", step.id));
    output.push_str(&format!("Plan ID: {}\n", step.plan_id));
    output.push_str(&format!("Status: {}\n", step.status));
    output.push_str(&format!("Executor: {}\n", step.executor));
    output.push_str(&format!("Content: {}\n", step.content));
    if has_text(&step.comment) {
        output.push_str(&format!(
            "Comment: {}\n",
            step.comment.as_deref().unwrap_or("")
        ));
    }
    output.push_str(&format!("Created: {}\n", format_datetime(step.created_at)));
    output.push_str(&format!("Updated: {}\n", format_datetime(step.updated_at)));
    output.push('\n');
    if goals.is_empty() {
        output.push_str("Goals: (none)");
        return output;
    }
    output.push_str("Goals:\n");
    for goal in goals {
        output.push_str(&format!(
            "- [{}] {} (goal id {})\n",
            goal.status, goal.content, goal.id
        ));
        if has_text(&goal.comment) {
            output.push_str(&format!(
                "  Comment: {}\n",
                goal.comment.as_deref().unwrap_or("")
            ));
        }
    }
    output.trim_end().to_string()
}

pub fn format_goal_detail(goal: &goal::Model, step: &step::Model) -> String {
    let mut output = String::new();
    output.push_str(&format!("Goal ID: {}\n", goal.id));
    output.push_str(&format!("Step ID: {}\n", goal.step_id));
    output.push_str(&format!("Plan ID: {}\n", step.plan_id));
    output.push_str(&format!("Status: {}\n", goal.status));
    output.push_str(&format!("Content: {}\n", goal.content));
    if has_text(&goal.comment) {
        output.push_str(&format!(
            "Comment: {}\n",
            goal.comment.as_deref().unwrap_or("")
        ));
    }
    output.push_str(&format!("Created: {}\n", format_datetime(goal.created_at)));
    output.push_str(&format!("Updated: {}\n", format_datetime(goal.updated_at)));
    output.push('\n');
    output.push_str(&format!("Step Status: {}\n", step.status));
    output.push_str(&format!("Step Executor: {}\n", step.executor));
    output.push_str(&format!("Step Content: {}\n", step.content));
    if has_text(&step.comment) {
        output.push_str(&format!(
            "Step Comment: {}\n",
            step.comment.as_deref().unwrap_or("")
        ));
    }
    output.trim_end().to_string()
}

pub fn format_plan_detail(
    plan: &plan::Model,
    steps: &[step::Model],
    goals: &HashMap<i64, Vec<goal::Model>>,
) -> String {
    let mut output = String::new();
    output.push_str(&format!("Plan ID: {}\n", plan.id));
    output.push_str(&format!("Title: {}\n", plan.title));
    output.push_str(&format!("Status: {}\n", plan.status));
    output.push_str(&format!("Content: {}\n", plan.content));
    if has_text(&plan.comment) {
        output.push_str(&format!(
            "Comment: {}\n",
            plan.comment.as_deref().unwrap_or("")
        ));
    }
    output.push_str(&format!("Created: {}\n", format_datetime(plan.created_at)));
    output.push_str(&format!("Updated: {}\n", format_datetime(plan.updated_at)));
    output.push('\n');
    if steps.is_empty() {
        output.push_str("Steps: (none)");
        return output;
    }
    output.push_str("Steps:\n");
    for step in steps {
        let counts = goals.get(&step.id).map(|items| {
            let done = items
                .iter()
                .filter(|goal| goal.status == GoalStatus::Done.as_str())
                .count();
            (done, items.len())
        });
        if let Some((done, total)) = counts {
            output.push_str(&format!(
                "- [{}] {} (step id {}, exec {}, goals {}/{})\n",
                step.status, step.content, step.id, step.executor, done, total
            ));
        } else {
            output.push_str(&format!(
                "- [{}] {} (step id {}, exec {})\n",
                step.status, step.content, step.id, step.executor
            ));
        }
        if has_text(&step.comment) {
            output.push_str(&format!(
                "  Comment: {}\n",
                step.comment.as_deref().unwrap_or("")
            ));
        }
        if let Some(goal_list) = goals.get(&step.id) {
            for goal in goal_list {
                output.push_str(&format!(
                    "  - [{}] {} (goal id {})\n",
                    goal.status, goal.content, goal.id
                ));
                if has_text(&goal.comment) {
                    output.push_str(&format!(
                        "    Comment: {}\n",
                        goal.comment.as_deref().unwrap_or("")
                    ));
                }
            }
        }
    }
    output.trim_end().to_string()
}

pub fn format_plan_markdown(
    active: bool,
    active_updated: Option<DateTime<Utc>>,
    plan: &plan::Model,
    steps: &[step::Model],
    goals: &HashMap<i64, Vec<goal::Model>>,
) -> String {
    fn checkbox(status: &str) -> &'static str {
        if status == "done" {
            "x"
        } else {
            " "
        }
    }

    fn collapse_heading(text: &str) -> String {
        let normalized = text.replace("\r\n", "\n");
        let parts: Vec<&str> = normalized
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect();
        if parts.is_empty() {
            "(untitled)".to_string()
        } else {
            parts.join(" / ")
        }
    }

    fn split_task_text(text: &str) -> (String, Vec<String>) {
        let normalized = text.replace("\r\n", "\n");
        let lines: Vec<&str> = normalized.lines().collect();
        if lines.is_empty() {
            return ("(empty)".to_string(), Vec::new());
        }
        let mut first_idx = None;
        for (idx, line) in lines.iter().enumerate() {
            if !line.trim().is_empty() {
                first_idx = Some(idx);
                break;
            }
        }
        let Some(first_idx) = first_idx else {
            return ("(empty)".to_string(), Vec::new());
        };
        let first = lines[first_idx].to_string();
        let rest = lines
            .iter()
            .skip(first_idx + 1)
            .map(|line| line.to_string())
            .collect();
        (first, rest)
    }

    fn push_line(lines: &mut Vec<String>, indent: usize, text: &str) {
        let mut line = String::new();
        line.push_str(&" ".repeat(indent));
        line.push_str(text);
        lines.push(line);
    }

    fn push_blank(lines: &mut Vec<String>, indent: usize) {
        if indent == 0 {
            lines.push(String::new());
        } else {
            lines.push(" ".repeat(indent));
        }
    }

    let mut lines = Vec::new();
    push_line(&mut lines, 0, "# Plan");
    push_blank(&mut lines, 0);
    push_line(
        &mut lines,
        0,
        &format!("## Plan: {}", collapse_heading(&plan.title)),
    );
    push_blank(&mut lines, 0);

    push_line(
        &mut lines,
        0,
        &format!("- **Active:** `{}`", if active { "true" } else { "false" }),
    );
    push_line(&mut lines, 0, &format!("- **Plan ID:** `{}`", plan.id));
    push_line(&mut lines, 0, &format!("- **Status:** `{}`", plan.status));
    if has_text(&plan.comment) {
        push_line(
            &mut lines,
            0,
            &format!("- **Comment:** {}", plan.comment.as_deref().unwrap_or("")),
        );
    }
    if let Some(updated_at) = active_updated {
        push_line(
            &mut lines,
            0,
            &format!("- **Activated:** {}", format_datetime(updated_at)),
        );
    }
    push_line(
        &mut lines,
        0,
        &format!("- **Created:** {}", format_datetime(plan.created_at)),
    );
    push_line(
        &mut lines,
        0,
        &format!("- **Updated:** {}", format_datetime(plan.updated_at)),
    );
    let steps_done = steps.iter().filter(|step| step.status == "done").count();
    push_line(
        &mut lines,
        0,
        &format!("- **Steps:** {}/{}", steps_done, steps.len()),
    );
    push_blank(&mut lines, 0);

    push_line(&mut lines, 0, "### Plan Content");
    push_blank(&mut lines, 0);
    if plan.content.trim().is_empty() {
        push_line(&mut lines, 0, "*No content*");
    } else {
        let normalized = plan.content.replace("\r\n", "\n");
        for line in normalized.lines() {
            if line.is_empty() {
                push_line(&mut lines, 0, ">");
            } else {
                push_line(&mut lines, 0, &format!("> {}", line));
            }
        }
    }
    push_blank(&mut lines, 0);

    push_line(&mut lines, 0, "### Steps");
    push_blank(&mut lines, 0);
    if steps.is_empty() {
        push_line(&mut lines, 0, "*No steps*");
        return lines.join("\n").trim_end().to_string();
    }

    for (idx, step) in steps.iter().enumerate() {
        let (first_line, rest_lines) = split_task_text(&step.content);
        push_line(
            &mut lines,
            0,
            &format!(
                "- [{}] **{}** *(id: {}, exec: {}, order: {})*",
                checkbox(&step.status),
                first_line,
                step.id,
                step.executor,
                step.sort_order
            ),
        );

        let mut has_rest = false;
        for line in rest_lines {
            if line.trim().is_empty() {
                continue;
            }
            if !has_rest {
                push_blank(&mut lines, 2);
                has_rest = true;
            } else {
                push_blank(&mut lines, 2);
            }
            push_line(&mut lines, 2, &line);
        }

        push_blank(&mut lines, 2);
        push_line(
            &mut lines,
            2,
            &format!("- Created: {}", format_datetime(step.created_at)),
        );
        push_line(
            &mut lines,
            2,
            &format!("- Updated: {}", format_datetime(step.updated_at)),
        );
        if has_text(&step.comment) {
            push_line(
                &mut lines,
                2,
                &format!("- Comment: {}", step.comment.as_deref().unwrap_or("")),
            );
        }

        match goals.get(&step.id) {
            Some(items) if !items.is_empty() => {
                let done = items
                    .iter()
                    .filter(|goal| goal.status == GoalStatus::Done.as_str())
                    .count();
                push_line(&mut lines, 2, &format!("- Goals: {done}/{}", items.len()));

                for goal in items {
                    let (goal_first, goal_rest) = split_task_text(&goal.content);
                    push_blank(&mut lines, 2);
                    push_line(
                        &mut lines,
                        2,
                        &format!(
                            "- [{}] {} *(id: {})*",
                            checkbox(&goal.status),
                            goal_first,
                            goal.id
                        ),
                    );
                    for line in goal_rest {
                        if line.trim().is_empty() {
                            continue;
                        }
                        push_blank(&mut lines, 4);
                        push_line(&mut lines, 4, &line);
                    }
                    if has_text(&goal.comment) {
                        push_blank(&mut lines, 4);
                        push_line(
                            &mut lines,
                            4,
                            &format!("Comment: {}", goal.comment.as_deref().unwrap_or("")),
                        );
                    }
                }
            }
            _ => {
                push_line(&mut lines, 2, "- Goals: 0/0");
                push_blank(&mut lines, 2);
                push_line(&mut lines, 2, "- (none)");
            }
        }

        if idx + 1 < steps.len() {
            push_blank(&mut lines, 0);
        }
    }

    lines.join("\n").trim_end().to_string()
}
