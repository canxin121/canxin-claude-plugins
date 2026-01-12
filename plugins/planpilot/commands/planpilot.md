---
name: planpilot
description: Use when the user asks to plan or break down work into steps/goals, track progress, manage plans, or mentions Planpilot/planpilot/plan pilot/计划/任务拆解/步骤/目标/进度.
argument-hint: <plan|step|goal> [...args]
allowed-tools: ["Bash(planpilot:*)"]
---

# Planpilot (Claude Code Plugin)

## Binary CLI Tool Name
`planpilot`

## Database location:
`<project>/.claude/.planpilot/planpilot.db`

## Hierarchy
- Plan contains steps; step contains goals.
- Goals are the smallest units of work; steps group goals; plans group steps.

## AI Workflow Guidelines
- Use Planpilot (this CLI + stop hook) for all planning, status, and progress tracking; do not use the built-in plan tool or any other method (including inspecting files, calling other MCP tools, or using other skills) to get plan/step/goal status.
- Do not use the built-in plan or todo tools (including TodoWrite/TodoRead). Always use Planpilot.
- If the CLI is missing (e.g., `planpilot: command not found`), ask the user whether to install it, and offer `/planpilot-install` if they agree.
- Record implementation details using Planpilot comments (plan/step/goal `--comment` or `comment` commands), and before starting a step or goal, think through what you are about to do next and capture that context in comments so the plan stays actionable.
- Before starting a new plan, perform deep thinking, investigation, and analysis; then create the plan with clear steps and goals.
- When creating a plan or step, prefer `add-tree` to define all steps/goals upfront and add everything in one pass.
- Prefer assigning steps to `ai`; only assign `human` for truly critical/high-risk items or when passwords, `sudo` access, irreversible git history rewrites, or remote git changes are required. If `human` steps are necessary, batch them, make the step content explicit about what the human must do, and only ask the user for input when the next step is assigned to `human` (do not ask for preferences/reviews/approvals on basic items).
- You may adjust plans/steps/goals while executing, but only when necessary; avoid frequent or arbitrary changes.
- Update status promptly as work completes; mark goals as done, and let steps/plans auto-refresh unless a step/plan has no children (then use `step done`/`plan done`).
- In each reply turn, complete at most one step; do not advance multiple steps in a single response.

## Status Management
- Status values: `todo`, `done`.
- Goals are manual (`goal done`); steps/plans auto-refresh from child status, and use `step done`/`plan done` only when they have no children (`step done --all-goals` marks all goals done and then marks the step done). Auto status changes print as `Auto status updates:` with reasons.
- Parent status auto-flips to `todo` on incomplete child work and to `done` when all children are done. If a plan has 0 steps or a step has 0 goals, no auto-flip happens; use `plan done` / `step done` as needed.
- If the user completed a `human` step, verify/mark each goal and clearly list what remains.
- When a step becomes `done` and there is another pending step, the CLI will print the next-step instruction: for `ai`, end the turn so Planpilot can surface it; for `human`, show the step detail and tell the user to complete the goals, then end the turn. When a plan becomes `done` (automatic or manual), the CLI will prompt you to summarize completed results and end the turn.

## Active Plan Management
- Use `plan activate` / `plan deactivate` to manage, and no active plan means the plan is paused until reactivated. Plans auto-deactivate on `done` (manual or automatic) or removal.
- Each plan can be active in only one session at a time; use `plan activate --force` to take over. Default to no `--force`, and if activation fails due to another session, ask the user whether to take over.
- Use `plan show-active` to know which plan is active and get its details.

## Stop Hook Behavior
- Stop hooks run when Claude Code is about to finish a turn; they can approve completion or block it and inject a follow-up prompt into the same session.
- Planpilot's hook uses `approve` to let the turn finish, and `block` to re-prompt with the next AI step details.
- It approves when there is no active plan, or the next todo step is not assigned to `ai`.
- It blocks when the next todo step is assigned to `ai`, returning the step detail. The message always starts with `Planpilot (auto):` on the first line.
- If the AI receives a stop-hook message but lacks plan/step/goal context, it must use Planpilot commands (e.g., `plan show-active`, `plan show`, `step show`, `goal list`) to fetch the missing context before proceeding.

## ID Notes
- Plan/step/goal IDs are database IDs and may be non-contiguous or not start at 1; always use the actual IDs shown by `list`/`show`.

## Commands

### plan
- IMPORTANT: The AI must NOT pass `--cwd` or `--session-id` manually. These are auto-injected by the hook; passing them will conflict with the injected values.
- `plan add <title> <content>`: create a plan.
  - Output: `Created plan ID: <id>: <title>`.
- `plan add-tree <title> <content> --step <content> [--executor ai|human] [--goal <goal> ...] [--step <content> ...]`: create a plan with steps/goals in one command.
  - Output: `Created plan ID: <id>: <title> (steps: <n>, goals: <n>)`.
  - Repeatable groups: you can repeat the `--step ... [--executor ...] [--goal ...]` group multiple times.
  - Each `--executor` / `--goal` applies to the most recent `--step`.
  - Example:
    ```bash
    planpilot plan add-tree "Release v1.2" "Plan description" \
      --step "Cut release branch" --executor human --goal "Create branch" --goal "Tag base" \
      --step "Build artifacts" --executor ai --goal "Build packages"
    ```
  - Another example (3 steps, some without goals/executor):
    ```bash
    planpilot plan add-tree "Onboarding" "Setup plan" \
      --step "Create accounts" --goal "GitHub" --goal "Slack" \
      --step "Install tooling" --executor ai \
      --step "Read handbook"
    ```
- `plan list [--all] [--status todo|done] [--order id|title|created|updated] [--desc]`: list plans (defaults to `todo` unless `--all` or `--status` is set).
  - Output: prints a header line, then one line per plan with `ID STAT STEPS TITLE COMMENT` (`STEPS` is `done/total`); use `plan show` for full details.
  - Output (empty): `No plans found.`
- `plan show <id>`: prints plan details and nested steps/goals (includes ids for plan/step/goal).
  - Output: plan header includes `Plan ID: <id>`, `Title`, `Status`, `Content`, `Created`, `Updated`, and `Comment` when present.
  - Output: each step line includes step id and executor; progress (`goals done/total`) is shown only when the step has goals. Each goal line includes goal id.
- `plan export <id> <path>`: export plan details to a markdown file.
  - Output: `Exported plan ID: <id> to <path>`.
- `plan update <id> [--title <title>] [--content <content>] [--status todo|done] [--comment <comment>]`: update fields; `--status done` is allowed only when all steps are done or the plan has no steps.
  - Output: `Updated plan ID: <id>: <title>`.
  - Errors: multi-line `Error: Invalid input:` with `cannot mark plan done; next pending step:` on the next line, followed by the same step detail output as `step show`.
- `plan done <id>`: mark plan done (same rule as `plan update --status done`).
  - Output: `Plan ID: <id> marked done.`
  - Output (active plan): `Active plan deactivated because plan is done.`
  - Errors: multi-line `Error: Invalid input:` with `cannot mark plan done; next pending step:` on the next line, followed by the same step detail output as `step show`.
- `plan comment <id1> <comment1> [<id2> <comment2> ...]`: add or replace comments for one or more plans.
  - Output (single): `Updated plan comment for plan ID: <id>.`
  - Output (batch): `Updated plan comments for <n> plans.`
  - Each plan comment uses an `<id> <comment>` pair; you can provide multiple pairs in one call.
  - Example:
    ```bash
    planpilot plan comment 12 "high priority" 15 "waiting on input"
    ```
- `plan remove <id>`: remove plan (and its steps/goals).
  - Output: `Plan ID: <id> removed.`
- `plan activate <id> [--force]`: set the active plan.
  - Output: `Active plan set to <id>: <title>`.
  - `--force` takes over a plan already active in another session.
  - Errors: `Error: Invalid input: cannot activate plan; plan is done`.
  - Errors: `Error: Invalid input: plan id <id> is already active in session <session_id> (use --force to take over)`.
- `plan show-active`: prints the active plan details (same format as `plan show`).
  - Output: the same plan detail format as `plan show`.
  - Output (empty): `No active plan.`
  - Output (missing): `Active plan ID: <id> not found.`
- `plan deactivate`: unset the active plan (does not delete any plan).
  - Output: `Active plan deactivated.`

### step
- `step add <plan_id> <content1> [<content2> ...] [--at <pos>] [--executor ai|human]`: add steps.
  - Output (single): `Created step ID: <id> for plan ID: <plan_id>`.
  - Output (batch): `Created <n> steps for plan ID: <plan_id>`.
- `step add-tree <plan_id> <content> [--executor ai|human] [--goal <goal> ...]`: create one step with goals in one command.
  - Output: `Created step ID: <id> for plan ID: <plan_id> (goals: <n>)`.
  - Example:
    ```bash
    planpilot step add-tree 1 "Draft summary" \
      --executor ai --goal "Collect inputs" --goal "Write draft"
    ```
- `step list <plan_id> [--all] [--status todo|done] [--executor ai|human] [--limit N] [--offset N] [--count] [--order order|id|created] [--desc]`: list steps (defaults to `todo` unless `--all` or `--status` is set).
  - Output: prints a header line, then one line per step with `ID STAT EXEC GOALS CONTENT COMMENT` (`GOALS` is `done/total`); use `step show` for full details.
  - Output (count): `Total: <n>` when `--count` is set (no list output).
  - Output (empty): `No steps found for plan ID: <plan_id>.`
- `step show <id>`: prints a single step with full details and its nested goals (includes ids for step/goal).
  - Output: step header includes `Step ID: <id>`, `Plan ID`, `Status`, `Executor`, `Content`, `Created`, `Updated`, and `Comment` when present.
  - Output: lists all goals with `[status]` and goal id.
- `step show-next`: show the next pending step for the active plan (same format as `step show`).
  - Output (empty): `No active plan.` or `No pending step.`.
- `step update <id> [--content <content>] [--status todo|done] [--executor ai|human] [--comment <comment>]`: update fields; `--status done` is allowed only when all goals are done or the step has no goals.
  - Output: `Updated step ID: <id>.`.
  - Errors: `Error: Invalid input: cannot mark step done; next pending goal: <content> (id <id>)`.
- `step comment <id1> <comment1> [<id2> <comment2> ...]`: add or replace comments for one or more steps.
  - Output (single): `Updated step comments for plan ID: <plan_id>.`
  - Output (batch): `Updated step comments for <n> plans.`
  - Each step comment uses an `<id> <comment>` pair; you can provide multiple pairs in one call.
  - Example:
    ```bash
    planpilot step comment 45 "blocked by API" 46 "ready to start"
    ```
- `step done <id> [--all-goals]`: mark step done (same rule as `step update --status done`). Use `--all-goals` to mark all goals in the step done first, then mark the step done.
  - Output: `Step ID: <id> marked done.`
  - Errors: `Error: Invalid input: cannot mark step done; next pending goal: <content> (id <id>)`.
- `step move <id> --to <pos>`: reorder and print the same one-line list as `step list`.
  - Output: `Reordered steps for plan ID: <plan_id>:` + list.
- `step remove <id1> [<id2> ...]`: remove step(s).
  - Output (single): `Step ID: <id> removed.`
  - Output (batch): `Removed <n> steps.`
  - Errors: `Error: Not found: step id(s) not found: <id1>[, <id2> ...]`.

### goal
- `goal add <step_id> <content1> [<content2> ...]`: add goals to a step.
  - Output (single): `Created goal ID: <id> for step ID: <step_id>`.
  - Output (batch): `Created <n> goals for step ID: <step_id>`.
- `goal list <step_id> [--all] [--status todo|done] [--limit N] [--offset N] [--count]`: list goals (defaults to `todo` unless `--all` or `--status` is set).
  - Output: prints a header line, then one line per goal with `ID STAT CONTENT COMMENT`.
  - Output (count): `Total: <n>` when `--count` is set (no list output).
  - Output (empty): `No goals found for step ID: <step_id>.`
- `goal update <id> [--content <content>] [--status todo|done] [--comment <comment>]`: update fields.
  - Output: `Updated goal <id>.`
- `goal comment <id1> <comment1> [<id2> <comment2> ...]`: add or replace comments for one or more goals.
  - Output (single): `Updated goal comments for plan ID: <plan_id>.`
  - Output (batch): `Updated goal comments for <n> plans.`
  - Each goal comment uses an `<id> <comment>` pair; you can provide multiple pairs in one call.
  - Example:
    ```bash
    planpilot goal comment 78 "done" 81 "needs review"
    ```
- `goal done <id1> [<id2> ...]`: mark one or more goals done.
  - Output (single): `Goal ID: <id> marked done.`
  - Output (batch): `Goals marked done: <n>.`
- `goal remove <id1> [<id2> ...]`: remove goal(s).
  - Output (single): `Goal ID: <id> removed.`
  - Output (batch): `Removed <n> goals.`
  - Errors: `Error: Not found: goal id(s) not found: <id1>[, <id2> ...]`.
