mod app;
mod cli;
mod db;
mod entities;
mod error;
mod hooks;
mod model;
mod util;

use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use clap::{CommandFactory, FromArgMatches};
use clap::parser::ValueSource;
use serde::Deserialize;

use crate::app::{App, StatusChanges, StepInput};
use crate::cli::{
    Cli, Command, GoalAdd, GoalCommand, GoalComment, GoalDone, GoalList, GoalRemove, GoalShow,
    GoalStatusArg, GoalUpdate, HookCommand, PlanActivate, PlanAdd, PlanAddTree, PlanCommand,
    PlanComment, PlanDone, PlanExport, PlanList, PlanRemove, PlanSearch,
    PlanSearchFieldArg, PlanSearchModeArg, PlanShow, PlanStatusArg, PlanUpdate, StepAdd,
    StepAddTree, StepCommand, StepComment, StepDone, StepExecutorArg, StepList, StepMove,
    StepOrderArg, StepRemove, StepShow, StepSpec, StepStatusArg, StepUpdate,
};
use crate::error::AppError;
use crate::model::{
    GoalChanges, GoalQuery, GoalStatus, PlanChanges, PlanInput, PlanStatus, StepChanges,
    StepExecutor, StepOrder, StepQuery, StepStatus,
};
use crate::util::{
    format_goal_detail, format_plan_detail, format_plan_markdown, format_step_detail,
};

const CWD_FLAG: &str = "--cwd";
const SESSION_ID_FLAG: &str = "--session-id";
const CLAUDE_PLUGIN_ROOT_ENV: &str = "CLAUDE_PLUGIN_ROOT";

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("Error: {err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), AppError> {
    let matches = Cli::command().get_matches();
    let cwd_flag_present = matches.value_source("cwd") == Some(ValueSource::CommandLine);
    let Cli {
        command,
        cwd,
        session_id,
    } = Cli::from_arg_matches(&matches)
        .map_err(|err| AppError::InvalidInput(err.to_string()))?;

    match command {
        Command::Hook(command) => {
            handle_hook(command);
            return Ok(());
        }
        command => {
            let session_id = resolve_session_id(session_id)?;
            let claude_home = resolve_claude_home()?;
            let db_path = db::resolve_db_path(&claude_home);
            db::ensure_parent_dir(&db_path)?;
            let mut lock = db::open_lock(&db_path)?;
            let _guard = lock.write()?;

            let db = db::connect(&db_path).await?;
            db::ensure_schema(&db).await?;
            let app = App::new(db, session_id.clone());

            match command {
                Command::Plan(command) => {
                    let should_sync = matches!(
                        &command,
                        PlanCommand::Add(_)
                            | PlanCommand::AddTree(_)
                            | PlanCommand::Comment(_)
                            | PlanCommand::Update(_)
                            | PlanCommand::Done(_)
                            | PlanCommand::Remove(_)
                            | PlanCommand::Activate(_)
                            | PlanCommand::Deactivate(_)
                    );
                    let plan_ids = match command {
                        PlanCommand::List(args) => {
                            let context = PlanListContext {
                                cwd: cwd.as_deref(),
                                claude_home: &claude_home,
                                cwd_flag_present,
                            };
                            handle_plan_list(&app, args, &context).await?
                        }
                        PlanCommand::Search(args) => {
                            let context = PlanListContext {
                                cwd: cwd.as_deref(),
                                claude_home: &claude_home,
                                cwd_flag_present,
                            };
                            handle_plan_search(&app, args, &context).await?
                        }
                        command => handle_plan(&app, command).await?,
                    };
                    if should_sync {
                        sync_plan_md(&claude_home, &app, &plan_ids).await?;
                    }
                }
                Command::Step(command) => {
                    let should_sync = matches!(
                        &command,
                        StepCommand::Add(_)
                            | StepCommand::AddTree(_)
                            | StepCommand::Comment(_)
                            | StepCommand::Update(_)
                            | StepCommand::Done(_)
                            | StepCommand::Move(_)
                            | StepCommand::Remove(_)
                    );
                    let plan_ids = handle_step(&app, command).await?;
                    if should_sync {
                        sync_plan_md(&claude_home, &app, &plan_ids).await?;
                    }
                }
                Command::Goal(command) => {
                    let should_sync = matches!(
                        &command,
                        GoalCommand::Add(_)
                            | GoalCommand::Comment(_)
                            | GoalCommand::Update(_)
                            | GoalCommand::Done(_)
                            | GoalCommand::Remove(_)
                    );
                    let plan_ids = handle_goal(&app, command).await?;
                    if should_sync {
                        sync_plan_md(&claude_home, &app, &plan_ids).await?;
                    }
                }
                Command::Hook(_) => {}
            }
        }
    }

    Ok(())
}

fn handle_hook(command: HookCommand) {
    match command {
        HookCommand::PreToolUse => hooks::run_pretooluse_hook(),
        HookCommand::Stop => hooks::run_stop_hook(),
    }
}

async fn handle_plan(app: &App, command: PlanCommand) -> Result<Vec<i64>, AppError> {
    match command {
        PlanCommand::Add(args) => handle_plan_add(app, args).await,
        PlanCommand::AddTree(args) => handle_plan_add_tree(app, args).await,
        PlanCommand::List(_) => Err(AppError::InvalidInput(
            "plan list must be handled with list context".to_string(),
        )),
        PlanCommand::Search(_) => Err(AppError::InvalidInput(
            "plan search must be handled with list context".to_string(),
        )),
        PlanCommand::Show(args) => handle_plan_show(app, args).await,
        PlanCommand::Export(args) => handle_plan_export(app, args).await,
        PlanCommand::Comment(args) => handle_plan_comment(app, args).await,
        PlanCommand::Update(args) => handle_plan_update(app, args).await,
        PlanCommand::Done(args) => handle_plan_done(app, args).await,
        PlanCommand::Remove(args) => handle_plan_remove(app, args).await,
        PlanCommand::Activate(args) => handle_plan_activate(app, args).await,
        PlanCommand::Active(_) => handle_plan_active(app).await,
        PlanCommand::Deactivate(_) => handle_plan_deactivate(app).await,
    }
}

async fn handle_step(app: &App, command: StepCommand) -> Result<Vec<i64>, AppError> {
    match command {
        StepCommand::Add(args) => handle_step_add(app, args).await,
        StepCommand::AddTree(args) => handle_step_add_tree(app, args).await,
        StepCommand::List(args) => handle_step_list(app, args).await,
        StepCommand::Show(args) => handle_step_show(app, args).await,
        StepCommand::ShowNext(_) => handle_step_show_next(app).await,
        StepCommand::Comment(args) => handle_step_comment(app, args).await,
        StepCommand::Update(args) => handle_step_update(app, args).await,
        StepCommand::Done(args) => handle_step_done(app, args).await,
        StepCommand::Move(args) => handle_step_move(app, args).await,
        StepCommand::Remove(args) => handle_step_remove(app, args).await,
    }
}

async fn handle_goal(app: &App, command: GoalCommand) -> Result<Vec<i64>, AppError> {
    match command {
        GoalCommand::Add(args) => handle_goal_add(app, args).await,
        GoalCommand::List(args) => handle_goal_list(app, args).await,
        GoalCommand::Show(args) => handle_goal_show(app, args).await,
        GoalCommand::Comment(args) => handle_goal_comment(app, args).await,
        GoalCommand::Update(args) => handle_goal_update(app, args).await,
        GoalCommand::Done(args) => handle_goal_done(app, args).await,
        GoalCommand::Remove(args) => handle_goal_remove(app, args).await,
    }
}

async fn handle_plan_add(app: &App, args: PlanAdd) -> Result<Vec<i64>, AppError> {
    require_non_empty("plan content", &args.content)?;
    let plan = app
        .add_plan(PlanInput {
            title: args.title,
            content: args.content,
        })
        .await?;

    println!("Created plan ID: {}: {}", plan.id, plan.title);
    Ok(vec![plan.id])
}

async fn handle_plan_add_tree(app: &App, args: PlanAddTree) -> Result<Vec<i64>, AppError> {
    require_non_empty("plan title", &args.title)?;
    require_non_empty("plan content", &args.content)?;
    let specs = parse_plan_add_tree_steps(&args.args)?;
    if specs.is_empty() {
        return Err(AppError::InvalidInput(
            "plan add-tree requires at least one --step".to_string(),
        ));
    }

    let mut steps = Vec::with_capacity(specs.len());
    for spec in specs {
        require_non_empty("step content", &spec.content)?;
        let executor = spec
            .executor
            .map(step_executor_from_arg)
            .unwrap_or(StepExecutor::Ai);
        let mut goals = Vec::new();
        if let Some(items) = spec.goals {
            for goal in items {
                require_non_empty("goal content", &goal)?;
                goals.push(goal);
            }
        }
        steps.push(StepInput {
            content: spec.content,
            executor,
            goals,
        });
    }

    let (plan, step_count, goal_count) = app
        .add_plan_tree(
            PlanInput {
                title: args.title,
                content: args.content,
            },
            steps,
        )
        .await?;

    println!(
        "Created plan ID: {}: {} (steps: {}, goals: {})",
        plan.id, plan.title, step_count, goal_count
    );
    Ok(vec![plan.id])
}

struct PlanListContext<'a> {
    cwd: Option<&'a Path>,
    claude_home: &'a Path,
    cwd_flag_present: bool,
}

async fn handle_plan_list(
    app: &App,
    args: PlanList,
    context: &PlanListContext<'_>,
) -> Result<Vec<i64>, AppError> {
    let PlanList { all, project } = args;
    let desired = if all {
        None
    } else {
        Some(PlanStatus::Todo)
    };

    let cwd = require_cwd(context)?;
    let plans = app.list_plans(None, false).await?;
    if plans.is_empty() {
        println!("No plans found.");
        return Ok(Vec::new());
    }

    let mut filtered: Vec<_> = plans
        .into_iter()
        .filter(|plan| match desired {
            None => true,
            Some(status) => plan.status == status.as_str(),
        })
        .collect();

    if project {
        let session_ids = collect_session_ids_for_project(context.claude_home, &cwd)?;
        filtered.retain(|plan| {
            plan.last_session_id
                .as_ref()
                .is_some_and(|id| session_ids.contains(id))
        });
    }

    if filtered.is_empty() {
        println!("No plans found.");
        return Ok(Vec::new());
    }

    let details = app.get_plan_details(&filtered).await?;
    print_plan_list(&details);
    Ok(Vec::new())
}

async fn handle_plan_search(
    app: &App,
    args: PlanSearch,
    context: &PlanListContext<'_>,
) -> Result<Vec<i64>, AppError> {
    let PlanSearch {
        all,
        project,
        search,
        search_mode,
        search_field,
        match_case,
    } = args;
    let desired = if all {
        None
    } else {
        Some(PlanStatus::Todo)
    };

    let cwd = require_cwd(context)?;
    let plans = app.list_plans(None, false).await?;
    if plans.is_empty() {
        println!("No plans found.");
        return Ok(Vec::new());
    }

    let mut filtered: Vec<_> = plans
        .into_iter()
        .filter(|plan| match desired {
            None => true,
            Some(status) => plan.status == status.as_str(),
        })
        .collect();

    if project {
        let session_ids = collect_session_ids_for_project(context.claude_home, &cwd)?;
        filtered.retain(|plan| {
            plan.last_session_id
                .as_ref()
                .is_some_and(|id| session_ids.contains(id))
        });
    }

    if filtered.is_empty() {
        println!("No plans found.");
        return Ok(Vec::new());
    }

    let mut details = app.get_plan_details(&filtered).await?;
    let search = PlanSearchQuery::new(search, search_mode, search_field, match_case);
    if !search.has_terms() {
        return Err(AppError::InvalidInput(
            "plan search requires at least one --search".to_string(),
        ));
    }
    details.retain(|detail| plan_matches_search(detail, &search));

    if details.is_empty() {
        println!("No plans found.");
        return Ok(Vec::new());
    }

    print_plan_list(&details);
    Ok(Vec::new())
}

async fn handle_plan_show(app: &App, args: PlanShow) -> Result<Vec<i64>, AppError> {
    let detail = app.get_plan_detail(args.id).await?;
    println!(
        "{}",
        format_plan_detail(&detail.plan, &detail.steps, &detail.goals)
    );
    Ok(Vec::new())
}

async fn handle_plan_export(app: &App, args: PlanExport) -> Result<Vec<i64>, AppError> {
    let detail = app.get_plan_detail(args.id).await?;
    let active = app.get_active_plan().await?;
    let (is_active, activated_at) = match active {
        Some(state) if state.plan_id == detail.plan.id => (true, Some(state.updated_at)),
        _ => (false, None),
    };
    db::ensure_parent_dir(&args.path)?;
    let markdown = format_plan_markdown(
        is_active,
        activated_at,
        &detail.plan,
        &detail.steps,
        &detail.goals,
    );
    fs::write(&args.path, markdown)?;
    println!(
        "Exported plan ID: {} to {}",
        detail.plan.id,
        args.path.display()
    );
    Ok(Vec::new())
}

async fn handle_plan_comment(app: &App, args: PlanComment) -> Result<Vec<i64>, AppError> {
    let entries = parse_comment_pairs("plan", args.pairs)?;
    let plan_ids = app.comment_plans(entries).await?;
    if plan_ids.len() == 1 {
        println!("Updated plan comment for plan ID: {}.", plan_ids[0]);
    } else {
        println!("Updated plan comments for {} plans.", plan_ids.len());
    }
    Ok(plan_ids)
}

async fn handle_plan_update(app: &App, args: PlanUpdate) -> Result<Vec<i64>, AppError> {
    if let Some(content) = &args.content {
        require_non_empty("plan content", content)?;
    }
    let (plan, cleared) = app
        .update_plan_with_active_clear(
            args.id,
            PlanChanges {
                title: args.title,
                content: args.content,
                status: args.status.clone().map(plan_status_from_arg),
                comment: args.comment,
            },
        )
        .await?;

    println!("Updated plan ID: {}: {}", plan.id, plan.title);
    if cleared {
        println!("Active plan deactivated because plan is done.");
    }
    if plan.status == PlanStatus::Done.as_str() {
        notify_plan_completed(&plan);
    }
    Ok(vec![plan.id])
}

async fn handle_plan_done(app: &App, args: PlanDone) -> Result<Vec<i64>, AppError> {
    let (plan, cleared) = app
        .update_plan_with_active_clear(
            args.id,
            PlanChanges {
                status: Some(PlanStatus::Done),
                ..Default::default()
            },
        )
        .await?;
    println!("Plan ID: {} marked done.", plan.id);
    if cleared {
        println!("Active plan deactivated because plan is done.");
    }
    if plan.status == PlanStatus::Done.as_str() {
        notify_plan_completed(&plan);
    }
    Ok(vec![plan.id])
}

async fn handle_plan_remove(app: &App, args: PlanRemove) -> Result<Vec<i64>, AppError> {
    app.delete_plan(args.id).await?;
    println!("Plan ID: {} removed.", args.id);
    Ok(Vec::new())
}

async fn handle_plan_activate(app: &App, args: PlanActivate) -> Result<Vec<i64>, AppError> {
    let plan = app.get_plan(args.id).await?;
    if plan.status == PlanStatus::Done.as_str() {
        return Err(AppError::InvalidInput(
            "cannot activate plan; plan is done".to_string(),
        ));
    }
    app.set_active_plan(plan.id, args.force).await?;
    println!("Active plan set to {}: {}", plan.id, plan.title);
    Ok(vec![plan.id])
}

async fn handle_plan_active(app: &App) -> Result<Vec<i64>, AppError> {
    let Some(state) = app.get_active_plan().await? else {
        println!("No active plan.");
        return Ok(Vec::new());
    };

    let detail = match app.get_plan_detail(state.plan_id).await {
        Ok(value) => value,
        Err(AppError::NotFound(_)) => {
            app.clear_active_plan().await?;
            println!("Active plan ID: {} not found.", state.plan_id);
            return Ok(Vec::new());
        }
        Err(err) => return Err(err),
    };
    println!(
        "{}",
        format_plan_detail(&detail.plan, &detail.steps, &detail.goals)
    );
    Ok(Vec::new())
}

async fn handle_plan_deactivate(app: &App) -> Result<Vec<i64>, AppError> {
    let active = app.get_active_plan().await?;
    app.clear_active_plan().await?;
    println!("Active plan deactivated.");
    Ok(active.map(|state| state.plan_id).into_iter().collect())
}

async fn handle_step_add(app: &App, args: StepAdd) -> Result<Vec<i64>, AppError> {
    if args.contents.is_empty() {
        return Err(AppError::InvalidInput("no contents provided".to_string()));
    }
    for content in &args.contents {
        require_non_empty("step content", content)?;
    }
    if let Some(at) = args.at {
        if at == 0 {
            return Err(AppError::InvalidInput("position starts at 1".to_string()));
        }
    }
    let (steps, changes) = app
        .add_steps_batch(
            args.plan_id,
            args.contents.clone(),
            StepStatus::Todo,
            step_executor_from_arg(args.executor),
            args.at,
        )
        .await?;
    if steps.len() == 1 {
        println!(
            "Created step ID: {} for plan ID: {}",
            steps[0].id, steps[0].plan_id
        );
    } else {
        println!(
            "Created {} steps for plan ID: {}",
            steps.len(),
            args.plan_id
        );
    }
    print_status_changes(&changes);
    Ok(vec![args.plan_id])
}

async fn handle_step_add_tree(app: &App, args: StepAddTree) -> Result<Vec<i64>, AppError> {
    require_non_empty("step content", &args.content)?;
    for goal in &args.goals {
        require_non_empty("goal content", goal)?;
    }
    let executor = args
        .executor
        .map(step_executor_from_arg)
        .unwrap_or(StepExecutor::Ai);
    let (step, goals, changes) = app
        .add_step_tree(args.plan_id, args.content, executor, args.goals)
        .await?;
    let goal_count = goals.len();

    println!(
        "Created step ID: {} for plan ID: {} (goals: {})",
        step.id, step.plan_id, goal_count
    );
    print_status_changes(&changes);
    notify_after_step_changes(app, &changes).await?;
    notify_plans_completed(app, &changes).await?;
    Ok(vec![step.plan_id])
}

async fn handle_step_list(app: &App, args: StepList) -> Result<Vec<i64>, AppError> {
    let status = if args.all {
        None
    } else if let Some(status) = args.status {
        Some(step_status_from_arg(status))
    } else {
        Some(StepStatus::Todo)
    };

    let query = StepQuery {
        status,
        executor: args.executor.map(step_executor_from_arg),
        limit: args.limit,
        offset: args.offset,
        order: args.order.map(step_order_from_arg),
        desc: args.desc,
    };

    if args.count {
        let total = app.count_steps(args.plan_id, &query).await?;
        println!("Total: {}", total);
        return Ok(Vec::new());
    }

    let steps = app.list_steps_filtered(args.plan_id, &query).await?;
    if steps.is_empty() {
        println!("No steps found for plan ID: {}.", args.plan_id);
        return Ok(Vec::new());
    }

    let details = app.get_steps_detail(&steps).await?;
    print_step_list(&details);
    Ok(Vec::new())
}

async fn handle_step_show(app: &App, args: StepShow) -> Result<Vec<i64>, AppError> {
    let detail = app.get_step_detail(args.id).await?;
    println!("{}", format_step_detail(&detail.step, &detail.goals));
    Ok(Vec::new())
}

async fn handle_step_show_next(app: &App) -> Result<Vec<i64>, AppError> {
    let Some(active) = app.get_active_plan().await? else {
        println!("No active plan.");
        return Ok(Vec::new());
    };
    let next = app.next_step(active.plan_id).await?;
    let Some(step) = next else {
        println!("No pending step.");
        return Ok(Vec::new());
    };
    let goals = app.goals_for_step(step.id).await?;
    println!("{}", format_step_detail(&step, &goals));
    Ok(Vec::new())
}

async fn handle_step_update(app: &App, args: StepUpdate) -> Result<Vec<i64>, AppError> {
    if let Some(content) = &args.content {
        require_non_empty("step content", content)?;
    }
    let status = args.status.map(step_status_from_arg);
    let (step, changes) = app
        .update_step(
            args.id,
            StepChanges {
                content: args.content,
                status,
                executor: args.executor.map(step_executor_from_arg),
                comment: args.comment,
            },
        )
        .await?;

    println!("Updated step ID: {}.", step.id);
    print_status_changes(&changes);
    if matches!(status, Some(StepStatus::Done)) && step.status == StepStatus::Done.as_str() {
        notify_next_step_for_plan(app, step.plan_id).await?;
    }
    notify_plans_completed(app, &changes).await?;
    Ok(vec![step.plan_id])
}

async fn handle_step_comment(app: &App, args: StepComment) -> Result<Vec<i64>, AppError> {
    let entries = parse_comment_pairs("step", args.pairs)?;
    let plan_ids = app.comment_steps(entries).await?;
    if plan_ids.len() == 1 {
        println!("Updated step comments for plan ID: {}.", plan_ids[0]);
    } else {
        println!("Updated step comments for {} plans.", plan_ids.len());
    }
    Ok(plan_ids)
}

async fn handle_step_done(app: &App, args: StepDone) -> Result<Vec<i64>, AppError> {
    let (step, changes) = app
        .set_step_done_with_goals(args.id, args.all_goals)
        .await?;
    println!("Step ID: {} marked done.", step.id);
    print_status_changes(&changes);
    notify_next_step_for_plan(app, step.plan_id).await?;
    notify_plans_completed(app, &changes).await?;
    Ok(vec![step.plan_id])
}

async fn handle_step_move(app: &App, args: StepMove) -> Result<Vec<i64>, AppError> {
    if args.to == 0 {
        return Err(AppError::InvalidInput("position starts at 1".to_string()));
    }
    let steps = app.move_step(args.id, args.to).await?;
    println!("Reordered steps for plan ID: {}:", steps[0].plan_id);
    let details = app.get_steps_detail(&steps).await?;
    print_step_list(&details);
    Ok(vec![steps[0].plan_id])
}

async fn handle_step_remove(app: &App, args: StepRemove) -> Result<Vec<i64>, AppError> {
    if args.ids.is_empty() {
        return Err(AppError::InvalidInput("no step ids provided".to_string()));
    }
    let plan_ids = app.plan_ids_for_steps(&args.ids).await?;
    let (deleted, changes) = app.delete_steps(&args.ids).await?;
    if args.ids.len() == 1 {
        println!("Step ID: {} removed.", args.ids[0]);
    } else {
        println!("Removed {} steps.", deleted);
    }
    print_status_changes(&changes);
    Ok(plan_ids)
}

async fn handle_goal_add(app: &App, args: GoalAdd) -> Result<Vec<i64>, AppError> {
    if args.contents.is_empty() {
        return Err(AppError::InvalidInput("no contents provided".to_string()));
    }
    for content in &args.contents {
        require_non_empty("goal content", content)?;
    }
    let (goals, changes) = app
        .add_goals_batch(args.step_id, args.contents.clone(), GoalStatus::Todo)
        .await?;
    if goals.len() == 1 {
        println!(
            "Created goal ID: {} for step ID: {}",
            goals[0].id, goals[0].step_id
        );
    } else {
        println!(
            "Created {} goals for step ID: {}",
            goals.len(),
            args.step_id
        );
    }
    print_status_changes(&changes);
    notify_after_step_changes(app, &changes).await?;
    notify_plans_completed(app, &changes).await?;
    let step = app.get_step(args.step_id).await?;
    Ok(vec![step.plan_id])
}

async fn handle_goal_list(app: &App, args: GoalList) -> Result<Vec<i64>, AppError> {
    let status = if args.all {
        None
    } else if let Some(status) = args.status {
        Some(goal_status_from_arg(status))
    } else {
        Some(GoalStatus::Todo)
    };

    let query = GoalQuery {
        status,
        limit: args.limit,
        offset: args.offset,
    };

    if args.count {
        let total = app.count_goals(args.step_id, &query).await?;
        println!("Total: {}", total);
        return Ok(Vec::new());
    }

    let goals = app.list_goals_filtered(args.step_id, &query).await?;
    if goals.is_empty() {
        println!("No goals found for step ID: {}.", args.step_id);
        return Ok(Vec::new());
    }

    print_goal_list(&goals);
    Ok(Vec::new())
}

async fn handle_goal_show(app: &App, args: GoalShow) -> Result<Vec<i64>, AppError> {
    let detail = app.get_goal_detail(args.id).await?;
    println!("{}", format_goal_detail(&detail.goal, &detail.step));
    Ok(Vec::new())
}

async fn handle_goal_update(app: &App, args: GoalUpdate) -> Result<Vec<i64>, AppError> {
    if let Some(content) = &args.content {
        require_non_empty("goal content", content)?;
    }
    let (goal, changes) = app
        .update_goal(
            args.id,
            GoalChanges {
                content: args.content,
                status: args.status.map(goal_status_from_arg),
                comment: args.comment,
            },
        )
        .await?;

    println!("Updated goal {}.", goal.id);
    print_status_changes(&changes);
    notify_after_step_changes(app, &changes).await?;
    notify_plans_completed(app, &changes).await?;
    let step = app.get_step(goal.step_id).await?;
    Ok(vec![step.plan_id])
}

async fn handle_goal_comment(app: &App, args: GoalComment) -> Result<Vec<i64>, AppError> {
    let entries = parse_comment_pairs("goal", args.pairs)?;
    let plan_ids = app.comment_goals(entries).await?;
    if plan_ids.len() == 1 {
        println!("Updated goal comments for plan ID: {}.", plan_ids[0]);
    } else {
        println!("Updated goal comments for {} plans.", plan_ids.len());
    }
    Ok(plan_ids)
}

async fn handle_goal_done(app: &App, args: GoalDone) -> Result<Vec<i64>, AppError> {
    if args.ids.len() == 1 {
        let (goal, changes) = app.set_goal_status(args.ids[0], GoalStatus::Done).await?;
        println!("Goal ID: {} marked done.", goal.id);
        print_status_changes(&changes);
        notify_after_step_changes(app, &changes).await?;
        notify_plans_completed(app, &changes).await?;
        let step = app.get_step(goal.step_id).await?;
        return Ok(vec![step.plan_id]);
    }

    let plan_ids = app.plan_ids_for_goals(&args.ids).await?;
    let (updated, changes) = app.set_goals_status(&args.ids, GoalStatus::Done).await?;
    println!("Goals marked done: {}.", updated);
    print_status_changes(&changes);
    notify_after_step_changes(app, &changes).await?;
    notify_plans_completed(app, &changes).await?;
    Ok(plan_ids)
}

async fn handle_goal_remove(app: &App, args: GoalRemove) -> Result<Vec<i64>, AppError> {
    if args.ids.is_empty() {
        return Err(AppError::InvalidInput("no goal ids provided".to_string()));
    }
    let plan_ids = app.plan_ids_for_goals(&args.ids).await?;
    let (deleted, changes) = app.delete_goals(&args.ids).await?;
    if args.ids.len() == 1 {
        println!("Goal ID: {} removed.", args.ids[0]);
    } else {
        println!("Removed {} goals.", deleted);
    }
    print_status_changes(&changes);
    notify_after_step_changes(app, &changes).await?;
    notify_plans_completed(app, &changes).await?;
    Ok(plan_ids)
}

async fn sync_plan_md(claude_home: &Path, app: &App, plan_ids: &[i64]) -> Result<(), AppError> {
    if plan_ids.is_empty() {
        return Ok(());
    }

    let active = app.get_active_plan().await?;
    let (active_id, active_updated) = match active {
        Some(state) => (Some(state.plan_id), Some(state.updated_at)),
        None => (None, None),
    };

    let mut seen = HashSet::new();
    for plan_id in plan_ids {
        if !seen.insert(*plan_id) {
            continue;
        }
        let detail = match app.get_plan_detail(*plan_id).await {
            Ok(detail) => detail,
            Err(AppError::NotFound(_)) => continue,
            Err(err) => return Err(err),
        };

        let is_active = active_id == Some(*plan_id);
        let activated_at = if is_active { active_updated } else { None };
        let md_path = db::resolve_plan_md_path(claude_home, *plan_id);
        db::ensure_parent_dir(&md_path)?;
        let markdown = format_plan_markdown(
            is_active,
            activated_at,
            &detail.plan,
            &detail.steps,
            &detail.goals,
        );
        fs::write(md_path, markdown)?;
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct HistoryEntry {
    project: Option<String>,
    #[serde(rename = "sessionId")]
    session_id: Option<String>,
}

#[derive(Clone, Debug)]
struct PlanSearchQuery {
    terms: Vec<String>,
    mode: PlanSearchModeArg,
    field: PlanSearchFieldArg,
    match_case: bool,
}

impl PlanSearchQuery {
    fn new(
        raw_terms: Vec<String>,
        search_mode: Option<PlanSearchModeArg>,
        search_field: Option<PlanSearchFieldArg>,
        match_case: bool,
    ) -> Self {
        let mut terms: Vec<String> = raw_terms
            .into_iter()
            .map(|term| term.trim().to_string())
            .filter(|term| !term.is_empty())
            .collect();
        if !match_case {
            terms = terms.into_iter().map(|term| term.to_lowercase()).collect();
        }
        PlanSearchQuery {
            terms,
            mode: search_mode.unwrap_or(PlanSearchModeArg::All),
            field: search_field.unwrap_or(PlanSearchFieldArg::Plan),
            match_case,
        }
    }

    fn has_terms(&self) -> bool {
        !self.terms.is_empty()
    }
}

fn plan_matches_search(detail: &crate::app::PlanDetail, search: &PlanSearchQuery) -> bool {
    let mut haystacks: Vec<String> = Vec::new();
    let add_value = |haystacks: &mut Vec<String>, value: &str, match_case: bool| {
        if match_case {
            haystacks.push(value.to_string());
        } else {
            haystacks.push(value.to_lowercase());
        }
    };

    let include_plan = matches!(
        search.field,
        PlanSearchFieldArg::Plan | PlanSearchFieldArg::All
    );
    let include_title = matches!(
        search.field,
        PlanSearchFieldArg::Title | PlanSearchFieldArg::Plan | PlanSearchFieldArg::All
    );
    let include_content = matches!(
        search.field,
        PlanSearchFieldArg::Content | PlanSearchFieldArg::Plan | PlanSearchFieldArg::All
    );
    let include_comment = matches!(
        search.field,
        PlanSearchFieldArg::Comment | PlanSearchFieldArg::Plan | PlanSearchFieldArg::All
    );
    let include_steps = matches!(search.field, PlanSearchFieldArg::Steps | PlanSearchFieldArg::All);
    let include_goals = matches!(search.field, PlanSearchFieldArg::Goals | PlanSearchFieldArg::All);

    if include_plan || include_title {
        add_value(&mut haystacks, &detail.plan.title, search.match_case);
    }
    if include_plan || include_content {
        add_value(&mut haystacks, &detail.plan.content, search.match_case);
    }
    if include_plan || include_comment {
        if let Some(comment) = detail.plan.comment.as_deref() {
            add_value(&mut haystacks, comment, search.match_case);
        }
    }
    if include_steps {
        for step in &detail.steps {
            add_value(&mut haystacks, &step.content, search.match_case);
        }
    }
    if include_goals {
        for goals in detail.goals.values() {
            for goal in goals {
                add_value(&mut haystacks, &goal.content, search.match_case);
            }
        }
    }

    if haystacks.is_empty() {
        return false;
    }

    match search.mode {
        PlanSearchModeArg::Any => search
            .terms
            .iter()
            .any(|term| haystacks.iter().any(|value| value.contains(term))),
        PlanSearchModeArg::All => search
            .terms
            .iter()
            .all(|term| haystacks.iter().any(|value| value.contains(term))),
    }
}

fn collect_session_ids_for_project(
    claude_home: &Path,
    project: &Path,
) -> Result<HashSet<String>, AppError> {
    let history_path = claude_home.join("history.jsonl");
    let file = match fs::File::open(&history_path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(err) => return Err(err.into()),
    };
    let canonical = fs::canonicalize(project).ok();
    let project_raw = project.to_string_lossy().to_string();
    let project_canonical = canonical
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let mut sessions = HashSet::new();
    for line in BufReader::new(file).lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let entry: HistoryEntry = match serde_json::from_str(&line) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let Some(project) = entry.project else { continue };
        if project_matches_path(&project, &project_raw, project_canonical.as_deref()) {
            if let Some(session_id) = entry.session_id {
                sessions.insert(session_id);
            }
        }
    }

    Ok(sessions)
}

fn project_matches_path(project: &str, path_raw: &str, path_canonical: Option<&str>) -> bool {
    if project == path_raw {
        return true;
    }
    if let Some(canonical) = path_canonical {
        if project == canonical {
            return true;
        }
        if canonical.starts_with(&format!("{project}/")) {
            return true;
        }
    }
    if path_raw.starts_with(&format!("{project}/")) {
        return true;
    }
    false
}

fn resolve_claude_home() -> Result<PathBuf, AppError> {
    if let Ok(plugin_root) = std::env::var(CLAUDE_PLUGIN_ROOT_ENV) {
        if let Some(home) = find_claude_home(Path::new(&plugin_root)) {
            return Ok(home);
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(home) = find_claude_home(&exe_path) {
            return Ok(home);
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home).join(".claude");
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    Err(AppError::InvalidInput(
        "unable to resolve Claude home; set CLAUDE_PLUGIN_ROOT".to_string(),
    ))
}

fn find_claude_home(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(path) = current {
        if path.file_name().is_some_and(|name| name == ".claude") {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

fn require_cwd(context: &PlanListContext<'_>) -> Result<PathBuf, AppError> {
    if !context.cwd_flag_present {
        return Err(AppError::InvalidInput(format!("{CWD_FLAG} is required")));
    }
    resolve_cwd(context.cwd.map(|path| path.to_path_buf()))
}

fn resolve_cwd(cwd: Option<PathBuf>) -> Result<PathBuf, AppError> {
    let path = cwd.ok_or_else(|| AppError::InvalidInput(format!("{CWD_FLAG} is required")))?;
    let trimmed = path.as_os_str().to_string_lossy();
    if trimmed.trim().is_empty() {
        return Err(AppError::InvalidInput(format!("{CWD_FLAG} is empty")));
    }
    Ok(path)
}

fn resolve_session_id(session_id: Option<String>) -> Result<String, AppError> {
    let value = session_id
        .ok_or_else(|| AppError::InvalidInput(format!("{SESSION_ID_FLAG} is required")))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "{SESSION_ID_FLAG} is empty"
        )));
    }
    Ok(trimmed.to_string())
}

#[derive(Debug)]
struct StepSpecBuilder {
    content: String,
    executor: Option<StepExecutorArg>,
    goals: Vec<String>,
}

impl StepSpecBuilder {
    fn new(content: &str) -> Self {
        Self {
            content: content.to_string(),
            executor: None,
            goals: Vec::new(),
        }
    }

    fn into_spec(self) -> StepSpec {
        StepSpec {
            content: self.content,
            executor: self.executor,
            goals: if self.goals.is_empty() {
                None
            } else {
                Some(self.goals)
            },
        }
    }
}

fn parse_plan_add_tree_steps(args: &[String]) -> Result<Vec<StepSpec>, AppError> {
    if args.is_empty() {
        return Err(AppError::InvalidInput(
            "plan add-tree requires at least one --step".to_string(),
        ));
    }

    let mut steps = Vec::new();
    let mut current: Option<StepSpecBuilder> = None;
    let mut idx = 0;

    while idx < args.len() {
        match args[idx].as_str() {
            "--" => {
                idx += 1;
            }
            "--step" => {
                let value = args.get(idx + 1).ok_or_else(|| {
                    AppError::InvalidInput("plan add-tree --step requires a value".to_string())
                })?;
                if let Some(step) = current.take() {
                    steps.push(step.into_spec());
                }
                let builder = parse_step_spec_value(value)?;
                current = Some(builder);
                idx += 2;
            }
            "--executor" => {
                let value = args.get(idx + 1).ok_or_else(|| {
                    AppError::InvalidInput("plan add-tree --executor requires a value".to_string())
                })?;
                let executor = parse_step_executor_arg(value)?;
                match current.as_mut() {
                    Some(step) => {
                        step.executor = Some(executor);
                    }
                    None => {
                        return Err(AppError::InvalidInput(
                            "plan add-tree --executor must follow a --step".to_string(),
                        ));
                    }
                }
                idx += 2;
            }
            "--goal" => {
                let value = args.get(idx + 1).ok_or_else(|| {
                    AppError::InvalidInput("plan add-tree --goal requires a value".to_string())
                })?;
                match current.as_mut() {
                    Some(step) => {
                        step.goals.push(value.to_string());
                    }
                    None => {
                        return Err(AppError::InvalidInput(
                            "plan add-tree --goal must follow a --step".to_string(),
                        ));
                    }
                }
                idx += 2;
            }
            unexpected => {
                return Err(AppError::InvalidInput(format!(
                    "plan add-tree unexpected argument: {unexpected}"
                )));
            }
        }
    }

    if let Some(step) = current.take() {
        steps.push(step.into_spec());
    }

    if steps.is_empty() {
        return Err(AppError::InvalidInput(
            "plan add-tree requires at least one --step".to_string(),
        ));
    }

    Ok(steps)
}

fn parse_step_executor_arg(value: &str) -> Result<StepExecutorArg, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ai" => Ok(StepExecutorArg::Ai),
        "human" => Ok(StepExecutorArg::Human),
        _ => Err(AppError::InvalidInput(format!(
            "invalid executor '{value}', expected ai|human"
        ))),
    }
}

fn parse_step_spec_value(value: &str) -> Result<StepSpecBuilder, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::InvalidInput(
            "plan add-tree --step cannot be empty".to_string(),
        ));
    }
    if trimmed.starts_with('{') {
        return Err(AppError::InvalidInput(
            "plan add-tree no longer accepts JSON step specs; use --step <content> [--executor ai|human] [--goal <goal> ...]"
                .to_string(),
        ));
    }
    Ok(StepSpecBuilder::new(value))
}

fn parse_comment_pairs(kind: &str, pairs: Vec<String>) -> Result<Vec<(i64, String)>, AppError> {
    if pairs.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "{kind} comment requires <id> <comment> pairs"
        )));
    }

    if pairs.len() % 2 != 0 {
        return Err(AppError::InvalidInput(format!(
            "{kind} comment expects <id> <comment> pairs"
        )));
    }

    let mut parsed = Vec::with_capacity(pairs.len() / 2);
    let mut iter = pairs.into_iter();
    while let Some(id_value) = iter.next() {
        let comment = iter.next().unwrap_or_default();
        let id = id_value.parse::<i64>().map_err(|_| {
            AppError::InvalidInput(format!("{kind} comment id '{id_value}' is invalid"))
        })?;
        require_non_empty("comment", &comment)?;
        parsed.push((id, comment));
    }

    Ok(parsed)
}

fn plan_status_from_arg(arg: PlanStatusArg) -> PlanStatus {
    match arg {
        PlanStatusArg::Todo => PlanStatus::Todo,
        PlanStatusArg::Done => PlanStatus::Done,
    }
}

fn step_status_from_arg(arg: StepStatusArg) -> StepStatus {
    match arg {
        StepStatusArg::Todo => StepStatus::Todo,
        StepStatusArg::Done => StepStatus::Done,
    }
}

fn step_executor_from_arg(arg: StepExecutorArg) -> StepExecutor {
    match arg {
        StepExecutorArg::Ai => StepExecutor::Ai,
        StepExecutorArg::Human => StepExecutor::Human,
    }
}

fn goal_status_from_arg(arg: GoalStatusArg) -> GoalStatus {
    match arg {
        GoalStatusArg::Todo => GoalStatus::Todo,
        GoalStatusArg::Done => GoalStatus::Done,
    }
}

fn step_order_from_arg(arg: StepOrderArg) -> StepOrder {
    match arg {
        StepOrderArg::Order => StepOrder::Order,
        StepOrderArg::Id => StepOrder::Id,
        StepOrderArg::Created => StepOrder::Created,
    }
}

fn require_non_empty(label: &str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::InvalidInput(format!("{label} cannot be empty")));
    }
    Ok(())
}

fn print_status_changes(changes: &StatusChanges) {
    if changes.is_empty() {
        return;
    }

    println!("Auto status updates:");
    for change in &changes.steps {
        println!(
            "- Step ID: {} status auto-updated from {} to {} ({}).",
            change.step_id, change.from, change.to, change.reason
        );
    }
    for change in &changes.plans {
        println!(
            "- Plan ID: {} status auto-updated from {} to {} ({}).",
            change.plan_id, change.from, change.to, change.reason
        );
    }
    for change in &changes.active_plans_cleared {
        println!(
            "- Active plan deactivated for plan ID: {} ({}).",
            change.plan_id, change.reason
        );
    }
}

async fn notify_after_step_changes(app: &App, changes: &StatusChanges) -> Result<(), AppError> {
    let mut plan_ids = HashSet::new();
    for change in &changes.steps {
        if change.to == StepStatus::Done.as_str() {
            let step = app.get_step(change.step_id).await?;
            plan_ids.insert(step.plan_id);
        }
    }
    for plan_id in plan_ids {
        notify_next_step_for_plan(app, plan_id).await?;
    }
    Ok(())
}

async fn notify_plans_completed(app: &App, changes: &StatusChanges) -> Result<(), AppError> {
    let mut plan_ids = HashSet::new();
    for change in &changes.plans {
        if change.to == PlanStatus::Done.as_str() {
            plan_ids.insert(change.plan_id);
        }
    }
    for plan_id in plan_ids {
        let plan = app.get_plan(plan_id).await?;
        if plan.status == PlanStatus::Done.as_str() {
            notify_plan_completed(&plan);
        }
    }
    Ok(())
}

fn notify_plan_completed(plan: &crate::entities::plan::Model) {
    println!(
        "Plan ID: {} is complete. Summarize the completed results to the user, then end this turn.",
        plan.id
    );
}

async fn notify_next_step_for_plan(app: &App, plan_id: i64) -> Result<(), AppError> {
    let next = app.next_step(plan_id).await?;
    let Some(step) = next else {
        return Ok(());
    };
    if step.executor == StepExecutor::Ai.as_str() {
        println!(
            "Next step is assigned to ai (step ID: {}). Please end this turn so Planpilot can surface it.",
            step.id
        );
        return Ok(());
    }

    let goals = app.goals_for_step(step.id).await?;
    println!("Next step requires human action:");
    println!("{}", format_step_detail(&step, &goals));
    println!(
        "Tell the user to complete the above step and goals. Confirm each goal when done, then end this turn."
    );
    Ok(())
}

fn print_plan_list(details: &[crate::app::PlanDetail]) {
    println!(
        "{:<4} {:<6} {:<7} {:<30} {}",
        "ID", "STAT", "STEPS", "TITLE", "COMMENT"
    );
    for detail in details {
        let total = detail.steps.len();
        let done = detail
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Done.as_str())
            .count();
        println!(
            "{:<4} {:<6} {:<7} {:<30} {}",
            detail.plan.id,
            detail.plan.status,
            format!("{}/{}", done, total),
            detail.plan.title,
            detail.plan.comment.as_deref().unwrap_or("")
        );
    }
}

fn print_step_list(details: &[crate::app::StepDetail]) {
    println!(
        "{:<4} {:<6} {:<6} {:<9} {:<30} {}",
        "ID", "STAT", "EXEC", "GOALS", "CONTENT", "COMMENT"
    );
    for detail in details {
        let total = detail.goals.len();
        let done = detail
            .goals
            .iter()
            .filter(|goal| goal.status == GoalStatus::Done.as_str())
            .count();
        println!(
            "{:<4} {:<6} {:<6} {:<9} {:<30} {}",
            detail.step.id,
            detail.step.status,
            detail.step.executor,
            format!("{}/{}", done, total),
            detail.step.content,
            detail.step.comment.as_deref().unwrap_or("")
        );
    }
}

fn print_goal_list(goals: &[crate::entities::goal::Model]) {
    println!("{:<4} {:<6} {:<30} {}", "ID", "STAT", "CONTENT", "COMMENT");
    for goal in goals {
        println!(
            "{:<4} {:<6} {:<30} {}",
            goal.id,
            goal.status,
            goal.content,
            goal.comment.as_deref().unwrap_or("")
        );
    }
}
