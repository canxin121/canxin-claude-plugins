use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

#[derive(Parser, Debug)]
#[command(
    name = "planpilot",
    version,
    about = "Manage plans and steps with SQLite"
)]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "PATH",
        help = "Current working directory (used for plan list scoping)"
    )]
    pub cwd: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "ID",
        help = "Session identifier"
    )]
    pub session_id: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(subcommand)]
    Plan(PlanCommand),
    #[command(subcommand)]
    Step(StepCommand),
    #[command(subcommand)]
    Goal(GoalCommand),
    #[command(subcommand)]
    Hook(HookCommand),
}

#[derive(Subcommand, Debug)]
pub enum PlanCommand {
    Add(PlanAdd),
    #[command(name = "add-tree")]
    AddTree(PlanAddTree),
    List(PlanList),
    Search(PlanSearch),
    Show(PlanShow),
    Export(PlanExport),
    Comment(PlanComment),
    Update(PlanUpdate),
    Done(PlanDone),
    Remove(PlanRemove),
    Activate(PlanActivate),
    #[command(name = "show-active")]
    Active(PlanActive),
    Deactivate(PlanDeactivate),
}

#[derive(Subcommand, Debug)]
pub enum StepCommand {
    Add(StepAdd),
    #[command(name = "add-tree")]
    AddTree(StepAddTree),
    List(StepList),
    Show(StepShow),
    #[command(name = "show-next")]
    ShowNext(StepShowNext),
    Comment(StepComment),
    Update(StepUpdate),
    Done(StepDone),
    Move(StepMove),
    Remove(StepRemove),
}

#[derive(Subcommand, Debug)]
pub enum GoalCommand {
    Add(GoalAdd),
    List(GoalList),
    Show(GoalShow),
    Comment(GoalComment),
    Update(GoalUpdate),
    Done(GoalDone),
    Remove(GoalRemove),
}

#[derive(Subcommand, Debug)]
pub enum HookCommand {
    #[command(name = "pretooluse")]
    PreToolUse,
    Stop,
}

#[derive(Args, Debug)]
pub struct PlanAdd {
    pub title: String,
    pub content: String,
}

#[derive(Args, Debug)]
pub struct PlanAddTree {
    pub title: String,
    pub content: String,
    #[arg(
        value_name = "ARGS",
        num_args = 1..,
        trailing_var_arg = true,
        allow_hyphen_values = true,
        help = "Use --step <content> [--executor ai|human] [--goal <goal> ...] repeating per step"
    )]
    pub args: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct StepSpec {
    pub content: String,
    pub executor: Option<StepExecutorArg>,
    pub goals: Option<Vec<String>>,
}

#[derive(Args, Debug)]
pub struct PlanList {
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub project: bool,
}

#[derive(Args, Debug)]
pub struct PlanSearch {
    #[arg(long)]
    pub all: bool,
    #[arg(long)]
    pub project: bool,
    #[arg(long, value_name = "TERM")]
    pub search: Vec<String>,
    #[arg(long, value_enum)]
    pub search_mode: Option<PlanSearchModeArg>,
    #[arg(long, value_enum)]
    pub search_field: Option<PlanSearchFieldArg>,
    #[arg(long)]
    pub match_case: bool,
}

#[derive(Args, Debug)]
pub struct PlanShow {
    pub id: i64,
}

#[derive(Args, Debug)]
pub struct PlanExport {
    pub id: i64,
    pub path: PathBuf,
}

#[derive(Args, Debug)]
pub struct PlanUpdate {
    pub id: i64,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub content: Option<String>,
    #[arg(long, value_enum)]
    pub status: Option<PlanStatusArg>,
    #[arg(long)]
    pub comment: Option<String>,
}

#[derive(Args, Debug)]
pub struct PlanDone {
    pub id: i64,
}

#[derive(Args, Debug)]
pub struct PlanRemove {
    pub id: i64,
}

#[derive(Args, Debug)]
pub struct PlanActivate {
    pub id: i64,
    #[arg(
        long,
        help = "Allow taking over a plan already active in another session"
    )]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct PlanActive {}

#[derive(Args, Debug)]
pub struct PlanDeactivate {}

#[derive(Args, Debug)]
pub struct StepAdd {
    pub plan_id: i64,
    #[arg(value_name = "CONTENT", num_args = 1..)]
    pub contents: Vec<String>,
    #[arg(long)]
    pub at: Option<usize>,
    #[arg(long, value_enum, default_value = "ai")]
    pub executor: StepExecutorArg,
}

#[derive(Args, Debug)]
pub struct StepAddTree {
    pub plan_id: i64,
    pub content: String,
    #[arg(long, value_enum)]
    pub executor: Option<StepExecutorArg>,
    #[arg(long = "goal", value_name = "GOAL")]
    pub goals: Vec<String>,
}

#[derive(Args, Debug)]
pub struct StepList {
    pub plan_id: i64,
    #[arg(long)]
    pub all: bool,
    #[arg(long, value_enum)]
    pub status: Option<StepStatusArg>,
    #[arg(long, value_enum)]
    pub executor: Option<StepExecutorArg>,
    #[arg(long)]
    pub limit: Option<u64>,
    #[arg(long)]
    pub offset: Option<u64>,
    #[arg(long)]
    pub count: bool,
    #[arg(long, value_enum)]
    pub order: Option<StepOrderArg>,
    #[arg(long)]
    pub desc: bool,
}

#[derive(Args, Debug)]
pub struct StepShow {
    pub id: i64,
}

#[derive(Args, Debug)]
pub struct StepShowNext {}

#[derive(Args, Debug)]
pub struct StepUpdate {
    pub id: i64,
    #[arg(long)]
    pub content: Option<String>,
    #[arg(long, value_enum)]
    pub status: Option<StepStatusArg>,
    #[arg(long, value_enum)]
    pub executor: Option<StepExecutorArg>,
    #[arg(long)]
    pub comment: Option<String>,
}

#[derive(Args, Debug)]
pub struct StepDone {
    pub id: i64,
    #[arg(long)]
    pub all_goals: bool,
}

#[derive(Args, Debug)]
pub struct StepMove {
    pub id: i64,
    #[arg(long)]
    pub to: usize,
}

#[derive(Args, Debug)]
pub struct StepRemove {
    #[arg(value_name = "ID", num_args = 1..)]
    pub ids: Vec<i64>,
}

#[derive(Args, Debug)]
pub struct GoalAdd {
    pub step_id: i64,
    #[arg(value_name = "CONTENT", num_args = 1..)]
    pub contents: Vec<String>,
}

#[derive(Args, Debug)]
pub struct GoalList {
    pub step_id: i64,
    #[arg(long)]
    pub all: bool,
    #[arg(long, value_enum)]
    pub status: Option<GoalStatusArg>,
    #[arg(long)]
    pub limit: Option<u64>,
    #[arg(long)]
    pub offset: Option<u64>,
    #[arg(long)]
    pub count: bool,
}

#[derive(Args, Debug)]
pub struct GoalShow {
    pub id: i64,
}

#[derive(Args, Debug)]
pub struct GoalUpdate {
    pub id: i64,
    #[arg(long)]
    pub content: Option<String>,
    #[arg(long, value_enum)]
    pub status: Option<GoalStatusArg>,
    #[arg(long)]
    pub comment: Option<String>,
}

#[derive(Args, Debug)]
pub struct PlanComment {
    #[arg(value_name = "ARG", num_args = 2..)]
    pub pairs: Vec<String>,
}

#[derive(Args, Debug)]
pub struct StepComment {
    #[arg(value_name = "ARG", num_args = 2..)]
    pub pairs: Vec<String>,
}

#[derive(Args, Debug)]
pub struct GoalComment {
    #[arg(value_name = "ARG", num_args = 2..)]
    pub pairs: Vec<String>,
}

#[derive(Args, Debug)]
pub struct GoalDone {
    #[arg(value_name = "ID", num_args = 1..)]
    pub ids: Vec<i64>,
}

#[derive(Args, Debug)]
pub struct GoalRemove {
    #[arg(value_name = "ID", num_args = 1..)]
    pub ids: Vec<i64>,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum PlanStatusArg {
    Todo,
    Done,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum PlanSearchModeArg {
    Any,
    All,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum PlanSearchFieldArg {
    Plan,
    Title,
    Content,
    Comment,
    Steps,
    Goals,
    All,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum StepStatusArg {
    Todo,
    Done,
}

#[derive(ValueEnum, Clone, Debug, Deserialize)]
pub enum StepExecutorArg {
    Ai,
    Human,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum GoalStatusArg {
    Todo,
    Done,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum StepOrderArg {
    Order,
    Id,
    Created,
}
