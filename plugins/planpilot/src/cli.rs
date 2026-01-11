use std::path::PathBuf;
use std::str::FromStr;

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
        help = "Project root directory (required)"
    )]
    pub cwd: Option<PathBuf>,
    #[arg(
        long,
        global = true,
        value_name = "ID",
        help = "Session identifier (required)"
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
    #[arg(long = "step", value_name = "JSON", num_args = 1.., action = clap::ArgAction::Append)]
    pub steps: Vec<StepSpec>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StepSpec {
    pub content: String,
    pub executor: Option<StepExecutorArg>,
    pub goals: Option<Vec<String>>,
}

impl FromStr for StepSpec {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }
}

#[derive(Args, Debug)]
pub struct PlanList {
    #[arg(long)]
    pub all: bool,
    #[arg(long, value_enum)]
    pub status: Option<PlanStatusArg>,
    #[arg(long, value_enum)]
    pub order: Option<PlanOrderArg>,
    #[arg(long)]
    pub desc: bool,
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
    #[arg(long = "entry", value_name = "JSON", num_args = 1.., action = clap::ArgAction::Append)]
    pub entries: Vec<CommentEntry>,
}

#[derive(Args, Debug)]
pub struct StepComment {
    #[arg(long = "entry", value_name = "JSON", num_args = 1.., action = clap::ArgAction::Append)]
    pub entries: Vec<CommentEntry>,
}

#[derive(Args, Debug)]
pub struct GoalComment {
    #[arg(long = "entry", value_name = "JSON", num_args = 1.., action = clap::ArgAction::Append)]
    pub entries: Vec<CommentEntry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommentEntry {
    pub id: i64,
    pub comment: String,
}

impl FromStr for CommentEntry {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(value).map_err(|err| err.to_string())
    }
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
pub enum PlanOrderArg {
    Id,
    Title,
    Created,
    Updated,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum StepOrderArg {
    Order,
    Id,
    Created,
}
