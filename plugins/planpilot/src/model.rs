use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PlanStatus {
    Todo,
    Done,
}

impl PlanStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StepStatus {
    Todo,
    Done,
}

impl StepStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum GoalStatus {
    Todo,
    Done,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Done => "done",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum StepExecutor {
    Ai,
    Human,
}

impl StepExecutor {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ai => "ai",
            Self::Human => "human",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlanInput {
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlanChanges {
    pub title: Option<String>,
    pub content: Option<String>,
    pub status: Option<PlanStatus>,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StepChanges {
    pub content: Option<String>,
    pub status: Option<StepStatus>,
    pub executor: Option<StepExecutor>,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StepQuery {
    pub status: Option<StepStatus>,
    pub executor: Option<StepExecutor>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub order: Option<StepOrder>,
    pub desc: bool,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum PlanOrder {
    Id,
    Title,
    Created,
    Updated,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum StepOrder {
    Order,
    Id,
    Created,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GoalChanges {
    pub content: Option<String>,
    pub status: Option<GoalStatus>,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GoalQuery {
    pub status: Option<GoalStatus>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}
