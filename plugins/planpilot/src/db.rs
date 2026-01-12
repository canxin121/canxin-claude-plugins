use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use sea_orm::sea_query::Index;
use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DatabaseConnection, Schema, Statement};
use url::Url;

use crate::entities::{active_plan, goal, plan, step};
use crate::error::AppError;

pub fn resolve_db_path(claude_home: &Path) -> PathBuf {
    resolve_planpilot_dir(claude_home).join("planpilot.db")
}

pub fn resolve_planpilot_dir(claude_home: &Path) -> PathBuf {
    claude_home.join(".planpilot")
}

pub fn resolve_plan_md_dir(claude_home: &Path) -> PathBuf {
    resolve_planpilot_dir(claude_home).join("plans")
}

pub fn resolve_plan_md_path(claude_home: &Path, plan_id: i64) -> PathBuf {
    resolve_plan_md_dir(claude_home).join(format!("plan_{plan_id}.md"))
}

pub fn ensure_parent_dir(path: &Path) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn open_lock(path: &Path) -> Result<fd_lock::RwLock<File>, AppError> {
    let lock_path = path.with_extension("lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(lock_path)?;
    Ok(fd_lock::RwLock::new(file))
}

pub async fn connect(path: &Path) -> Result<DatabaseConnection, AppError> {
    let mut url = Url::from_file_path(path)
        .map_err(|_| AppError::InvalidInput(format!("invalid sqlite path: {}", path.display())))?;
    url.set_query(Some("mode=rwc"));
    let sqlite_url = url.as_str().replacen("file://", "sqlite://", 1);
    Ok(Database::connect(&sqlite_url).await?)
}

pub async fn ensure_schema(db: &DatabaseConnection) -> Result<(), AppError> {
    db.execute(Statement::from_string(
        DatabaseBackend::Sqlite,
        "PRAGMA foreign_keys = ON;",
    ))
    .await?;

    let builder = db.get_database_backend();
    let schema = Schema::new(builder);

    let mut plan_stmt = schema.create_table_from_entity(plan::Entity);
    plan_stmt.if_not_exists();
    db.execute(builder.build(&plan_stmt)).await?;

    let mut step_stmt = schema.create_table_from_entity(step::Entity);
    step_stmt.if_not_exists();
    db.execute(builder.build(&step_stmt)).await?;

    let mut goal_stmt = schema.create_table_from_entity(goal::Entity);
    goal_stmt.if_not_exists();
    db.execute(builder.build(&goal_stmt)).await?;

    let mut active_stmt = schema.create_table_from_entity(active_plan::Entity);
    active_stmt.if_not_exists();
    db.execute(builder.build(&active_stmt)).await?;

    let builder = db.get_database_backend();

    let mut index_stmt = Index::create()
        .name("idx_steps_plan_order")
        .table(step::Entity)
        .col(step::Column::PlanId)
        .col(step::Column::SortOrder)
        .to_owned();
    index_stmt.if_not_exists();
    db.execute(builder.build(&index_stmt)).await?;

    let mut goal_index = Index::create()
        .name("idx_goals_step")
        .table(goal::Entity)
        .col(goal::Column::StepId)
        .to_owned();
    goal_index.if_not_exists();
    db.execute(builder.build(&goal_index)).await?;

    let mut active_index = Index::create()
        .name("idx_active_plan_session")
        .table(active_plan::Entity)
        .col(active_plan::Column::SessionId)
        .unique()
        .to_owned();
    active_index.if_not_exists();
    db.execute(builder.build(&active_index)).await?;

    let mut active_plan_index = Index::create()
        .name("idx_active_plan_plan")
        .table(active_plan::Entity)
        .col(active_plan::Column::PlanId)
        .unique()
        .to_owned();
    active_plan_index.if_not_exists();
    db.execute(builder.build(&active_plan_index)).await?;

    Ok(())
}
