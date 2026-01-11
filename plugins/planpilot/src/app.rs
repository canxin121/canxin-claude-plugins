use std::collections::{HashMap, HashSet};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DatabaseTransaction,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Set, TransactionTrait,
};

use crate::entities::{active_plan, goal, plan, step};
use crate::error::AppError;
use crate::model::{
    GoalChanges, GoalQuery, GoalStatus, PlanChanges, PlanInput, PlanOrder, PlanStatus, StepChanges,
    StepExecutor, StepOrder, StepQuery, StepStatus,
};
use crate::util::format_step_detail;

pub struct App {
    db: DatabaseConnection,
    session_id: String,
}

pub struct StepDetail {
    pub step: step::Model,
    pub goals: Vec<goal::Model>,
}

pub struct GoalDetail {
    pub goal: goal::Model,
    pub step: step::Model,
}

#[derive(Clone, Debug)]
pub struct StepInput {
    pub content: String,
    pub executor: StepExecutor,
    pub goals: Vec<String>,
}

pub struct PlanDetail {
    pub plan: plan::Model,
    pub steps: Vec<step::Model>,
    pub goals: HashMap<i64, Vec<goal::Model>>,
}

#[derive(Clone, Debug)]
pub struct StepStatusChange {
    pub step_id: i64,
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct PlanStatusChange {
    pub plan_id: i64,
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct ActivePlanCleared {
    pub plan_id: i64,
    pub reason: String,
}

#[derive(Default, Debug)]
pub struct StatusChanges {
    pub steps: Vec<StepStatusChange>,
    pub plans: Vec<PlanStatusChange>,
    pub active_plans_cleared: Vec<ActivePlanCleared>,
}

impl StatusChanges {
    pub fn merge(&mut self, other: StatusChanges) {
        self.steps.extend(other.steps);
        self.plans.extend(other.plans);
        self.active_plans_cleared.extend(other.active_plans_cleared);
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty() && self.plans.is_empty() && self.active_plans_cleared.is_empty()
    }
}

impl App {
    pub fn new(db: DatabaseConnection, session_id: String) -> Self {
        Self { db, session_id }
    }

    pub async fn add_plan(&self, input: PlanInput) -> Result<plan::Model, AppError> {
        ensure_non_empty("plan title", &input.title)?;
        ensure_non_empty("plan content", &input.content)?;
        let now = Utc::now();
        let active = plan::ActiveModel {
            title: Set(input.title),
            content: Set(input.content),
            status: Set(PlanStatus::Todo.as_str().to_string()),
            last_session_id: Set(Some(self.session_id.clone())),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };

        let insert = plan::Entity::insert(active).exec(&self.db).await?;
        let created = plan::Entity::find_by_id(insert.last_insert_id)
            .one(&self.db)
            .await?;
        created.ok_or_else(|| AppError::NotFound("plan not found after insert".to_string()))
    }

    pub async fn add_plan_tree(
        &self,
        input: PlanInput,
        steps: Vec<StepInput>,
    ) -> Result<(plan::Model, usize, usize), AppError> {
        ensure_non_empty("plan title", &input.title)?;
        ensure_non_empty("plan content", &input.content)?;
        for step in &steps {
            ensure_non_empty("step content", &step.content)?;
            for goal in &step.goals {
                ensure_non_empty("goal content", goal)?;
            }
        }

        let txn = self.db.begin().await?;
        let result: Result<(plan::Model, usize, usize), AppError> = async {
            let now = Utc::now();
            let active_plan = plan::ActiveModel {
                title: Set(input.title),
                content: Set(input.content),
                status: Set(PlanStatus::Todo.as_str().to_string()),
                last_session_id: Set(Some(self.session_id.clone())),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };

            let insert = plan::Entity::insert(active_plan).exec(&txn).await?;
            let plan_model = plan::Entity::find_by_id(insert.last_insert_id)
                .one(&txn)
                .await?
                .ok_or_else(|| AppError::NotFound("plan not found after insert".to_string()))?;

            let mut step_count = 0usize;
            let mut goal_count = 0usize;
            for (idx, step_input) in steps.into_iter().enumerate() {
                let step_active = step::ActiveModel {
                    plan_id: Set(plan_model.id),
                    content: Set(step_input.content),
                    status: Set(StepStatus::Todo.as_str().to_string()),
                    executor: Set(step_input.executor.as_str().to_string()),
                    sort_order: Set((idx + 1) as i32),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                let insert = step::Entity::insert(step_active).exec(&txn).await?;
                let step_model = step::Entity::find_by_id(insert.last_insert_id)
                    .one(&txn)
                    .await?
                    .ok_or_else(|| AppError::NotFound("step not found after insert".to_string()))?;
                step_count += 1;

                if !step_input.goals.is_empty() {
                    for goal_content in step_input.goals {
                        let goal_active = goal::ActiveModel {
                            step_id: Set(step_model.id),
                            content: Set(goal_content),
                            status: Set(GoalStatus::Todo.as_str().to_string()),
                            created_at: Set(now),
                            updated_at: Set(now),
                            ..Default::default()
                        };
                        goal::Entity::insert(goal_active).exec(&txn).await?;
                        goal_count += 1;
                    }
                }
            }

            Ok((plan_model, step_count, goal_count))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn list_plans(
        &self,
        order: Option<PlanOrder>,
        desc: bool,
    ) -> Result<Vec<plan::Model>, AppError> {
        let mut select = plan::Entity::find();
        let order = order.unwrap_or(PlanOrder::Updated);
        match (order, desc) {
            (PlanOrder::Id, true) => select = select.order_by_desc(plan::Column::Id),
            (PlanOrder::Id, false) => select = select.order_by_asc(plan::Column::Id),
            (PlanOrder::Title, true) => select = select.order_by_desc(plan::Column::Title),
            (PlanOrder::Title, false) => select = select.order_by_asc(plan::Column::Title),
            (PlanOrder::Created, true) => select = select.order_by_desc(plan::Column::CreatedAt),
            (PlanOrder::Created, false) => select = select.order_by_asc(plan::Column::CreatedAt),
            (PlanOrder::Updated, true) => select = select.order_by_desc(plan::Column::UpdatedAt),
            (PlanOrder::Updated, false) => select = select.order_by_asc(plan::Column::UpdatedAt),
        }
        Ok(select.order_by_asc(plan::Column::Id).all(&self.db).await?)
    }

    pub async fn get_plan(&self, id: i64) -> Result<plan::Model, AppError> {
        plan::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("plan id {id}")))
    }

    pub async fn get_step(&self, id: i64) -> Result<step::Model, AppError> {
        step::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("step id {id}")))
    }

    pub async fn get_goal(&self, id: i64) -> Result<goal::Model, AppError> {
        goal::Entity::find_by_id(id)
            .one(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("goal id {id}")))
    }

    pub async fn plan_with_steps(
        &self,
        id: i64,
    ) -> Result<(plan::Model, Vec<step::Model>), AppError> {
        let plan = self.get_plan(id).await?;

        let steps = step::Entity::find()
            .filter(step::Column::PlanId.eq(id))
            .order_by_asc(step::Column::SortOrder)
            .order_by_asc(step::Column::Id)
            .all(&self.db)
            .await?;

        Ok((plan, steps))
    }

    pub async fn get_plan_detail(&self, id: i64) -> Result<PlanDetail, AppError> {
        let (plan, steps) = self.plan_with_steps(id).await?;
        let step_ids: Vec<i64> = steps.iter().map(|step| step.id).collect();
        let goals = self.goals_for_steps(&step_ids).await?;
        Ok(PlanDetail { plan, steps, goals })
    }

    pub async fn get_step_detail(&self, id: i64) -> Result<StepDetail, AppError> {
        let step = self.get_step(id).await?;
        let goals = self.goals_for_step(step.id).await?;
        Ok(StepDetail { step, goals })
    }

    pub async fn get_goal_detail(&self, id: i64) -> Result<GoalDetail, AppError> {
        let goal = self.get_goal(id).await?;
        let step = self.get_step(goal.step_id).await?;
        Ok(GoalDetail { goal, step })
    }

    pub async fn get_plan_details(
        &self,
        plans: &[plan::Model],
    ) -> Result<Vec<PlanDetail>, AppError> {
        if plans.is_empty() {
            return Ok(Vec::new());
        }
        let plan_ids: Vec<i64> = plans.iter().map(|plan| plan.id).collect();
        let steps = step::Entity::find()
            .filter(step::Column::PlanId.is_in(plan_ids))
            .order_by_asc(step::Column::SortOrder)
            .order_by_asc(step::Column::Id)
            .all(&self.db)
            .await?;
        let step_ids: Vec<i64> = steps.iter().map(|step| step.id).collect();
        let goals_map = self.goals_for_steps(&step_ids).await?;

        let mut steps_by_plan: HashMap<i64, Vec<step::Model>> = HashMap::new();
        for step in steps {
            steps_by_plan.entry(step.plan_id).or_default().push(step);
        }

        let mut details = Vec::with_capacity(plans.len());
        for plan in plans {
            let steps = steps_by_plan.remove(&plan.id).unwrap_or_default();
            let mut goals = HashMap::new();
            for step in &steps {
                if let Some(items) = goals_map.get(&step.id) {
                    goals.insert(step.id, items.clone());
                }
            }
            details.push(PlanDetail {
                plan: plan.clone(),
                steps,
                goals,
            });
        }

        Ok(details)
    }

    pub async fn get_steps_detail(
        &self,
        steps: &[step::Model],
    ) -> Result<Vec<StepDetail>, AppError> {
        if steps.is_empty() {
            return Ok(Vec::new());
        }
        let step_ids: Vec<i64> = steps.iter().map(|step| step.id).collect();
        let goals_map = self.goals_for_steps(&step_ids).await?;
        let mut details = Vec::with_capacity(steps.len());
        for step in steps {
            let goals = goals_map.get(&step.id).cloned().unwrap_or_default();
            details.push(StepDetail {
                step: step.clone(),
                goals,
            });
        }
        Ok(details)
    }

    pub async fn get_active_plan(&self) -> Result<Option<active_plan::Model>, AppError> {
        Ok(active_plan::Entity::find()
            .filter(active_plan::Column::SessionId.eq(self.session_id.as_str()))
            .one(&self.db)
            .await?)
    }

    pub async fn set_active_plan(
        &self,
        plan_id: i64,
        takeover: bool,
    ) -> Result<active_plan::Model, AppError> {
        self.get_plan(plan_id).await?;
        let now = Utc::now();
        let txn = self.db.begin().await?;
        if let Some(existing) = active_plan::Entity::find()
            .filter(active_plan::Column::PlanId.eq(plan_id))
            .one(&txn)
            .await?
        {
            if existing.session_id != self.session_id && !takeover {
                txn.rollback().await?;
                return Err(AppError::InvalidInput(format!(
                    "plan id {plan_id} is already active in session {} (use --force to take over)",
                    existing.session_id
                )));
            }
        }
        active_plan::Entity::delete_many()
            .filter(active_plan::Column::SessionId.eq(self.session_id.as_str()))
            .exec(&txn)
            .await?;
        active_plan::Entity::delete_many()
            .filter(active_plan::Column::PlanId.eq(plan_id))
            .exec(&txn)
            .await?;

        let active = active_plan::ActiveModel {
            session_id: Set(self.session_id.clone()),
            plan_id: Set(plan_id),
            updated_at: Set(now),
            ..Default::default()
        };
        active_plan::Entity::insert(active).exec(&txn).await?;
        self.touch_plan_with_conn(&txn, plan_id).await?;
        let model = active_plan::Entity::find()
            .filter(active_plan::Column::SessionId.eq(self.session_id.as_str()))
            .one(&txn)
            .await?
            .ok_or_else(|| AppError::NotFound("active plan not found after insert".to_string()))?;
        txn.commit().await?;
        Ok(model)
    }

    pub async fn clear_active_plan(&self) -> Result<(), AppError> {
        active_plan::Entity::delete_many()
            .filter(active_plan::Column::SessionId.eq(self.session_id.as_str()))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    async fn clear_active_plans_for_plan_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        plan_id: i64,
    ) -> Result<bool, AppError> {
        let cleared_current = active_plan::Entity::find()
            .filter(active_plan::Column::PlanId.eq(plan_id))
            .filter(active_plan::Column::SessionId.eq(self.session_id.as_str()))
            .one(db)
            .await?
            .is_some();
        active_plan::Entity::delete_many()
            .filter(active_plan::Column::PlanId.eq(plan_id))
            .exec(db)
            .await?;
        Ok(cleared_current)
    }

    pub async fn update_plan_with_active_clear(
        &self,
        id: i64,
        changes: PlanChanges,
    ) -> Result<(plan::Model, bool), AppError> {
        let txn = self.db.begin().await?;
        let result: Result<(plan::Model, bool), AppError> = async {
            let plan = self.update_plan_with_conn(&txn, id, changes).await?;
            let cleared = if plan.status == PlanStatus::Done.as_str() {
                self.clear_active_plans_for_plan_with_conn(&txn, plan.id)
                    .await?
            } else {
                false
            };
            Ok((plan, cleared))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    async fn update_plan_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        id: i64,
        changes: PlanChanges,
    ) -> Result<plan::Model, AppError> {
        if let Some(title) = changes.title.as_deref() {
            ensure_non_empty("plan title", title)?;
        }
        if let Some(content) = changes.content.as_deref() {
            ensure_non_empty("plan content", content)?;
        }
        if let Some(status) = changes.status {
            if status == PlanStatus::Done {
                let total = step::Entity::find()
                    .filter(step::Column::PlanId.eq(id))
                    .count(db)
                    .await?;
                if total > 0 {
                    if let Some(pending) = self.next_step_with_conn(db, id).await? {
                        let goals = self.goals_for_step_with_conn(db, pending.id).await?;
                        let detail = format_step_detail(&pending, &goals);
                        return Err(AppError::InvalidInput(format!(
                            "cannot mark plan done; next pending step:\n{detail}"
                        )));
                    }
                }
            }
        }

        let mut active = plan::ActiveModel {
            id: Set(id),
            ..Default::default()
        };

        if let Some(title) = changes.title {
            active.title = Set(title);
        }
        if let Some(content) = changes.content {
            active.content = Set(content);
        }
        if let Some(status) = changes.status {
            active.status = Set(status.as_str().to_string());
        }
        if let Some(comment) = changes.comment {
            active.comment = Set(Some(comment));
        }
        active.last_session_id = Set(Some(self.session_id.clone()));

        active.updated_at = Set(Utc::now());

        match active.update(db).await {
            Ok(model) => Ok(model),
            Err(sea_orm::DbErr::RecordNotFound(_)) | Err(sea_orm::DbErr::RecordNotUpdated) => {
                Err(AppError::NotFound(format!("plan id {id}")))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub async fn delete_plan(&self, id: i64) -> Result<(), AppError> {
        let txn = self.db.begin().await?;
        active_plan::Entity::delete_many()
            .filter(active_plan::Column::PlanId.eq(id))
            .exec(&txn)
            .await?;
        let steps = step::Entity::find()
            .filter(step::Column::PlanId.eq(id))
            .all(&txn)
            .await?;
        let step_ids: Vec<i64> = steps.iter().map(|step| step.id).collect();
        if !step_ids.is_empty() {
            goal::Entity::delete_many()
                .filter(goal::Column::StepId.is_in(step_ids.clone()))
                .exec(&txn)
                .await?;
            step::Entity::delete_many()
                .filter(step::Column::PlanId.eq(id))
                .exec(&txn)
                .await?;
        }

        let result = plan::Entity::delete_by_id(id).exec(&txn).await?;
        if result.rows_affected == 0 {
            txn.rollback().await?;
            return Err(AppError::NotFound(format!("plan id {id}")));
        }
        txn.commit().await?;
        Ok(())
    }

    pub async fn goals_for_steps(
        &self,
        step_ids: &[i64],
    ) -> Result<HashMap<i64, Vec<goal::Model>>, AppError> {
        let mut grouped = HashMap::new();
        if step_ids.is_empty() {
            return Ok(grouped);
        }

        let goals = goal::Entity::find()
            .filter(goal::Column::StepId.is_in(step_ids.to_vec()))
            .order_by_asc(goal::Column::StepId)
            .order_by_asc(goal::Column::Id)
            .all(&self.db)
            .await?;

        for goal in goals {
            grouped
                .entry(goal.step_id)
                .or_insert_with(Vec::new)
                .push(goal);
        }

        Ok(grouped)
    }

    pub async fn goals_for_step(&self, step_id: i64) -> Result<Vec<goal::Model>, AppError> {
        self.goals_for_step_with_conn(&self.db, step_id).await
    }

    pub async fn add_steps_batch(
        &self,
        plan_id: i64,
        contents: Vec<String>,
        status: StepStatus,
        executor: StepExecutor,
        at: Option<usize>,
    ) -> Result<(Vec<step::Model>, StatusChanges), AppError> {
        let plan_exists = plan::Entity::find_by_id(plan_id).one(&self.db).await?;
        if plan_exists.is_none() {
            return Err(AppError::NotFound(format!("plan id {plan_id}")));
        }
        if contents.is_empty() {
            return Ok((Vec::new(), StatusChanges::default()));
        }
        for content in &contents {
            ensure_non_empty("step content", content)?;
        }

        let txn = self.db.begin().await?;
        let result: Result<(Vec<step::Model>, StatusChanges), AppError> = async {
            let mut existing = step::Entity::find()
                .filter(step::Column::PlanId.eq(plan_id))
                .order_by_asc(step::Column::SortOrder)
                .order_by_asc(step::Column::Id)
                .all(&txn)
                .await?;
            self.normalize_steps_in_place(&mut existing, &txn).await?;

            let total = existing.len();
            let insert_pos = match at {
                Some(pos) if pos > 0 => pos.min(total + 1),
                Some(_) => 1,
                None => total + 1,
            };

            let now = Utc::now();
            let shift_by = contents.len() as i32;
            if shift_by > 0 {
                for step_model in existing.iter_mut().rev() {
                    if step_model.sort_order >= insert_pos as i32 {
                        let mut active: step::ActiveModel = step_model.clone().into();
                        active.sort_order = Set(step_model.sort_order + shift_by);
                        active.updated_at = Set(now);
                        active.update(&txn).await?;
                        step_model.sort_order += shift_by;
                        step_model.updated_at = now;
                    }
                }
            }

            let mut created = Vec::with_capacity(contents.len());
            for (idx, content) in contents.into_iter().enumerate() {
                let sort_order = (insert_pos + idx) as i32;
                let active = step::ActiveModel {
                    plan_id: Set(plan_id),
                    content: Set(content),
                    status: Set(status.as_str().to_string()),
                    executor: Set(executor.as_str().to_string()),
                    sort_order: Set(sort_order),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                let insert = step::Entity::insert(active).exec(&txn).await?;
                let model = step::Entity::find_by_id(insert.last_insert_id)
                    .one(&txn)
                    .await?
                    .ok_or_else(|| AppError::NotFound("step not found after insert".to_string()))?;
                created.push(model);
            }

            let changes = self.refresh_plan_status_with_conn(&txn, plan_id).await?;
            self.touch_plan_with_conn(&txn, plan_id).await?;
            Ok((created, changes))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn add_step_tree(
        &self,
        plan_id: i64,
        content: String,
        executor: StepExecutor,
        goals: Vec<String>,
    ) -> Result<(step::Model, Vec<goal::Model>, StatusChanges), AppError> {
        ensure_non_empty("step content", &content)?;
        for goal in &goals {
            ensure_non_empty("goal content", goal)?;
        }

        let txn = self.db.begin().await?;
        let result: Result<(step::Model, Vec<goal::Model>, StatusChanges), AppError> = async {
            plan::Entity::find_by_id(plan_id)
                .one(&txn)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("plan id {plan_id}")))?;

            let mut existing = step::Entity::find()
                .filter(step::Column::PlanId.eq(plan_id))
                .order_by_asc(step::Column::SortOrder)
                .order_by_asc(step::Column::Id)
                .all(&txn)
                .await?;
            self.normalize_steps_in_place(&mut existing, &txn).await?;

            let sort_order = (existing.len() + 1) as i32;
            let now = Utc::now();
            let active = step::ActiveModel {
                plan_id: Set(plan_id),
                content: Set(content),
                status: Set(StepStatus::Todo.as_str().to_string()),
                executor: Set(executor.as_str().to_string()),
                sort_order: Set(sort_order),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            let insert = step::Entity::insert(active).exec(&txn).await?;
            let step_model = step::Entity::find_by_id(insert.last_insert_id)
                .one(&txn)
                .await?
                .ok_or_else(|| AppError::NotFound("step not found after insert".to_string()))?;

            let mut created_goals = Vec::new();
            for goal_content in goals {
                let goal_active = goal::ActiveModel {
                    step_id: Set(step_model.id),
                    content: Set(goal_content),
                    status: Set(GoalStatus::Todo.as_str().to_string()),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                let insert = goal::Entity::insert(goal_active).exec(&txn).await?;
                let goal_model = goal::Entity::find_by_id(insert.last_insert_id)
                    .one(&txn)
                    .await?
                    .ok_or_else(|| AppError::NotFound("goal not found after insert".to_string()))?;
                created_goals.push(goal_model);
            }

            let changes = self.refresh_plan_status_with_conn(&txn, plan_id).await?;
            self.touch_plan_with_conn(&txn, plan_id).await?;
            Ok((step_model, created_goals, changes))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn list_steps_filtered(
        &self,
        plan_id: i64,
        query: &StepQuery,
    ) -> Result<Vec<step::Model>, AppError> {
        self.get_plan(plan_id).await?;
        let mut select = step::Entity::find().filter(step::Column::PlanId.eq(plan_id));
        if let Some(status) = query.status {
            select = select.filter(step::Column::Status.eq(status.as_str()));
        }
        if let Some(executor) = query.executor {
            select = select.filter(step::Column::Executor.eq(executor.as_str()));
        }
        let order = query.order.unwrap_or(StepOrder::Order);
        match (order, query.desc) {
            (StepOrder::Order, true) => select = select.order_by_desc(step::Column::SortOrder),
            (StepOrder::Order, false) => select = select.order_by_asc(step::Column::SortOrder),
            (StepOrder::Id, true) => select = select.order_by_desc(step::Column::Id),
            (StepOrder::Id, false) => select = select.order_by_asc(step::Column::Id),
            (StepOrder::Created, true) => select = select.order_by_desc(step::Column::CreatedAt),
            (StepOrder::Created, false) => select = select.order_by_asc(step::Column::CreatedAt),
        }
        if let Some(limit) = query.limit {
            select = select.limit(limit);
        }
        if let Some(offset) = query.offset {
            select = select.offset(offset);
        }
        Ok(select.order_by_asc(step::Column::Id).all(&self.db).await?)
    }

    pub async fn next_step(&self, plan_id: i64) -> Result<Option<step::Model>, AppError> {
        self.next_step_with_conn(&self.db, plan_id).await
    }

    pub async fn count_steps(&self, plan_id: i64, query: &StepQuery) -> Result<u64, AppError> {
        self.get_plan(plan_id).await?;
        let mut select = step::Entity::find().filter(step::Column::PlanId.eq(plan_id));
        if let Some(status) = query.status {
            select = select.filter(step::Column::Status.eq(status.as_str()));
        }
        if let Some(executor) = query.executor {
            select = select.filter(step::Column::Executor.eq(executor.as_str()));
        }
        Ok(select.count(&self.db).await?)
    }

    pub async fn update_step(
        &self,
        id: i64,
        changes: StepChanges,
    ) -> Result<(step::Model, StatusChanges), AppError> {
        let txn = self.db.begin().await?;
        let result = self.update_step_with_conn(&txn, id, changes).await;
        finalize_transaction(txn, result).await
    }

    pub async fn set_step_done_with_goals(
        &self,
        id: i64,
        all_goals: bool,
    ) -> Result<(step::Model, StatusChanges), AppError> {
        let txn = self.db.begin().await?;
        let result: Result<(step::Model, StatusChanges), AppError> = async {
            let mut merged = StatusChanges::default();
            if all_goals {
                let changes = self.set_all_goals_done_for_step_with_conn(&txn, id).await?;
                merged.merge(changes);
            }
            let (step, changes) = self
                .update_step_with_conn(
                    &txn,
                    id,
                    StepChanges {
                        status: Some(StepStatus::Done),
                        ..Default::default()
                    },
                )
                .await?;
            merged.merge(changes);
            Ok((step, merged))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    async fn update_step_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        id: i64,
        changes: StepChanges,
    ) -> Result<(step::Model, StatusChanges), AppError> {
        if let Some(content) = changes.content.as_deref() {
            ensure_non_empty("step content", content)?;
        }
        if let Some(status) = changes.status {
            if status == StepStatus::Done {
                let goals = goal::Entity::find()
                    .filter(goal::Column::StepId.eq(id))
                    .count(db)
                    .await?;
                if goals > 0 {
                    if let Some(goal) = self.next_goal_for_step_with_conn(db, id).await? {
                        return Err(AppError::InvalidInput(format!(
                            "cannot mark step done; next pending goal: {} (id {})",
                            goal.content, goal.id
                        )));
                    }
                }
            }
        }

        let mut active = step::ActiveModel {
            id: Set(id),
            ..Default::default()
        };

        if let Some(content) = changes.content {
            active.content = Set(content);
        }
        if let Some(status) = changes.status {
            active.status = Set(status.as_str().to_string());
        }
        if let Some(executor) = changes.executor {
            active.executor = Set(executor.as_str().to_string());
        }
        if let Some(comment) = changes.comment {
            active.comment = Set(Some(comment));
        }

        active.updated_at = Set(Utc::now());

        match active.update(db).await {
            Ok(model) => {
                let mut updates = StatusChanges::default();
                if changes.status.is_some() {
                    let refreshed = self
                        .refresh_plan_status_with_conn(db, model.plan_id)
                        .await?;
                    updates.merge(refreshed);
                }
                self.touch_plan_with_conn(db, model.plan_id).await?;
                Ok((model, updates))
            }
            Err(sea_orm::DbErr::RecordNotFound(_)) | Err(sea_orm::DbErr::RecordNotUpdated) => {
                Err(AppError::NotFound(format!("step id {id}")))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub async fn delete_steps(&self, ids: &[i64]) -> Result<(u64, StatusChanges), AppError> {
        let txn = self.db.begin().await?;
        let result: Result<(u64, StatusChanges), AppError> = async {
            if ids.is_empty() {
                return Ok((0, StatusChanges::default()));
            }
            let unique_ids = unique_ids(ids);
            let steps = step::Entity::find()
                .filter(step::Column::Id.is_in(unique_ids.clone()))
                .all(&txn)
                .await?;
            let existing: HashSet<i64> = steps.iter().map(|step| step.id).collect();
            let missing: Vec<i64> = unique_ids
                .iter()
                .cloned()
                .filter(|id| !existing.contains(id))
                .collect();
            if !missing.is_empty() {
                return Err(AppError::NotFound(format!(
                    "step id(s) not found: {}",
                    join_ids(&missing)
                )));
            }
            let mut seen = HashSet::new();
            let mut plan_ids = Vec::new();
            for step in &steps {
                if seen.insert(step.plan_id) {
                    plan_ids.push(step.plan_id);
                }
            }

            goal::Entity::delete_many()
                .filter(goal::Column::StepId.is_in(unique_ids.clone()))
                .exec(&txn)
                .await?;
            let result = step::Entity::delete_many()
                .filter(step::Column::Id.is_in(unique_ids))
                .exec(&txn)
                .await?;
            for plan_id in &plan_ids {
                self.normalize_steps_for_plan(&txn, *plan_id).await?;
            }

            let mut changes = StatusChanges::default();
            for plan_id in &plan_ids {
                let updated = self.refresh_plan_status_with_conn(&txn, *plan_id).await?;
                changes.merge(updated);
            }
            if !plan_ids.is_empty() {
                self.touch_plans_with_conn(&txn, &plan_ids).await?;
            }

            Ok((result.rows_affected, changes))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn move_step(&self, id: i64, to: usize) -> Result<Vec<step::Model>, AppError> {
        let txn = self.db.begin().await?;
        let target = step::Entity::find_by_id(id)
            .one(&txn)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("step id {id}")))?;
        let plan_id = target.plan_id;

        let mut steps = step::Entity::find()
            .filter(step::Column::PlanId.eq(plan_id))
            .order_by_asc(step::Column::SortOrder)
            .order_by_asc(step::Column::Id)
            .all(&txn)
            .await?;

        let current_index = steps
            .iter()
            .position(|step| step.id == id)
            .ok_or_else(|| AppError::NotFound(format!("step id {id}")))?;

        let mut desired_index = to.saturating_sub(1);
        if desired_index >= steps.len() {
            desired_index = steps.len().saturating_sub(1);
        }

        let moving = steps.remove(current_index);
        if desired_index >= steps.len() {
            steps.push(moving);
        } else {
            steps.insert(desired_index, moving);
        }

        let now = Utc::now();
        for (idx, step_model) in steps.iter_mut().enumerate() {
            let desired_order = (idx + 1) as i32;
            if step_model.sort_order != desired_order {
                let mut active: step::ActiveModel = step_model.clone().into();
                active.sort_order = Set(desired_order);
                active.updated_at = Set(now);
                active.update(&txn).await?;
                step_model.sort_order = desired_order;
                step_model.updated_at = now;
            }
        }

        txn.commit().await?;
        Ok(steps)
    }

    async fn refresh_plan_status_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        plan_id: i64,
    ) -> Result<StatusChanges, AppError> {
        let total = step::Entity::find()
            .filter(step::Column::PlanId.eq(plan_id))
            .count(db)
            .await?;
        if total == 0 {
            return Ok(StatusChanges::default());
        }
        let done = step::Entity::find()
            .filter(step::Column::PlanId.eq(plan_id))
            .filter(step::Column::Status.eq(StepStatus::Done.as_str()))
            .count(db)
            .await?;
        let status = if done == total {
            PlanStatus::Done
        } else {
            PlanStatus::Todo
        };

        let plan = plan::Entity::find_by_id(plan_id).one(db).await?;
        let Some(plan) = plan else {
            return Err(AppError::NotFound(format!("plan {plan_id}")));
        };
        let mut changes = StatusChanges::default();
        if plan.status != status.as_str() {
            let reason = if done == total {
                format!("all steps are done ({done}/{total})")
            } else {
                format!("steps done {done}/{total}")
            };
            let mut active = plan::ActiveModel {
                id: Set(plan_id),
                ..Default::default()
            };
            active.status = Set(status.as_str().to_string());
            active.updated_at = Set(Utc::now());
            active.update(db).await?;
            changes.plans.push(PlanStatusChange {
                plan_id,
                from: plan.status,
                to: status.as_str().to_string(),
                reason,
            });
            if status == PlanStatus::Done {
                let cleared = self
                    .clear_active_plans_for_plan_with_conn(db, plan_id)
                    .await?;
                if cleared {
                    changes.active_plans_cleared.push(ActivePlanCleared {
                        plan_id,
                        reason: "plan marked done".to_string(),
                    });
                }
            }
        }

        Ok(changes)
    }

    async fn refresh_step_status_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        step_id: i64,
    ) -> Result<StatusChanges, AppError> {
        let goals = goal::Entity::find()
            .filter(goal::Column::StepId.eq(step_id))
            .all(db)
            .await?;
        if goals.is_empty() {
            return Ok(StatusChanges::default());
        }

        let done = goals
            .iter()
            .filter(|goal| goal.status == GoalStatus::Done.as_str())
            .count();
        let total = goals.len();
        let status = if done == total {
            StepStatus::Done
        } else {
            StepStatus::Todo
        };

        let step = step::Entity::find_by_id(step_id).one(db).await?;
        let Some(step) = step else {
            return Err(AppError::NotFound(format!("step {step_id}")));
        };
        let mut changes = StatusChanges::default();
        if step.status != status.as_str() {
            let mut active = step::ActiveModel {
                id: Set(step_id),
                ..Default::default()
            };
            active.status = Set(status.as_str().to_string());
            active.updated_at = Set(Utc::now());
            active.update(db).await?;
            let reason = if done == total {
                format!("all goals are done ({done}/{total})")
            } else {
                format!("goals done {done}/{total}")
            };
            changes.steps.push(StepStatusChange {
                step_id,
                from: step.status,
                to: status.as_str().to_string(),
                reason,
            });
        }

        let plan_changes = self.refresh_plan_status_with_conn(db, step.plan_id).await?;
        changes.merge(plan_changes);
        Ok(changes)
    }

    async fn next_step_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        plan_id: i64,
    ) -> Result<Option<step::Model>, AppError> {
        Ok(step::Entity::find()
            .filter(step::Column::PlanId.eq(plan_id))
            .filter(step::Column::Status.eq(StepStatus::Todo.as_str()))
            .order_by_asc(step::Column::SortOrder)
            .order_by_asc(step::Column::Id)
            .one(db)
            .await?)
    }

    async fn next_goal_for_step_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        step_id: i64,
    ) -> Result<Option<goal::Model>, AppError> {
        Ok(goal::Entity::find()
            .filter(goal::Column::StepId.eq(step_id))
            .filter(goal::Column::Status.eq(GoalStatus::Todo.as_str()))
            .order_by_asc(goal::Column::Id)
            .one(db)
            .await?)
    }

    async fn goals_for_step_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        step_id: i64,
    ) -> Result<Vec<goal::Model>, AppError> {
        Ok(goal::Entity::find()
            .filter(goal::Column::StepId.eq(step_id))
            .order_by_asc(goal::Column::Id)
            .all(db)
            .await?)
    }

    pub async fn add_goals_batch(
        &self,
        step_id: i64,
        contents: Vec<String>,
        status: GoalStatus,
    ) -> Result<(Vec<goal::Model>, StatusChanges), AppError> {
        if contents.is_empty() {
            return Ok((Vec::new(), StatusChanges::default()));
        }
        for content in &contents {
            ensure_non_empty("goal content", content)?;
        }

        let txn = self.db.begin().await?;
        let result: Result<(Vec<goal::Model>, StatusChanges), AppError> = async {
            let step = step::Entity::find_by_id(step_id)
                .one(&txn)
                .await?
                .ok_or_else(|| AppError::NotFound(format!("step id {step_id}")))?;
            let plan_id = step.plan_id;

            let now = Utc::now();
            let mut created = Vec::with_capacity(contents.len());
            for content in contents.into_iter() {
                let active = goal::ActiveModel {
                    step_id: Set(step_id),
                    content: Set(content),
                    status: Set(status.as_str().to_string()),
                    created_at: Set(now),
                    updated_at: Set(now),
                    ..Default::default()
                };
                let insert = goal::Entity::insert(active).exec(&txn).await?;
                let model = goal::Entity::find_by_id(insert.last_insert_id)
                    .one(&txn)
                    .await?
                    .ok_or_else(|| AppError::NotFound("goal not found after insert".to_string()))?;
                created.push(model);
            }

            let changes = self.refresh_step_status_with_conn(&txn, step_id).await?;
            self.touch_plan_with_conn(&txn, plan_id).await?;
            Ok((created, changes))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn list_goals_filtered(
        &self,
        step_id: i64,
        query: &GoalQuery,
    ) -> Result<Vec<goal::Model>, AppError> {
        self.get_step(step_id).await?;
        let mut select = goal::Entity::find().filter(goal::Column::StepId.eq(step_id));
        if let Some(status) = query.status {
            select = select.filter(goal::Column::Status.eq(status.as_str()));
        }
        if let Some(limit) = query.limit {
            select = select.limit(limit);
        }
        if let Some(offset) = query.offset {
            select = select.offset(offset);
        }
        Ok(select.order_by_asc(goal::Column::Id).all(&self.db).await?)
    }

    pub async fn count_goals(&self, step_id: i64, query: &GoalQuery) -> Result<u64, AppError> {
        self.get_step(step_id).await?;
        let mut select = goal::Entity::find().filter(goal::Column::StepId.eq(step_id));
        if let Some(status) = query.status {
            select = select.filter(goal::Column::Status.eq(status.as_str()));
        }
        Ok(select.count(&self.db).await?)
    }

    pub async fn plan_ids_for_steps(&self, ids: &[i64]) -> Result<Vec<i64>, AppError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let unique = unique_ids(ids);
        let steps = step::Entity::find()
            .filter(step::Column::Id.is_in(unique))
            .all(&self.db)
            .await?;
        let mut seen = HashSet::new();
        let mut plan_ids = Vec::new();
        for step_model in steps {
            if seen.insert(step_model.plan_id) {
                plan_ids.push(step_model.plan_id);
            }
        }
        Ok(plan_ids)
    }

    pub async fn plan_ids_for_goals(&self, ids: &[i64]) -> Result<Vec<i64>, AppError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let unique = unique_ids(ids);
        let goals = goal::Entity::find()
            .filter(goal::Column::Id.is_in(unique))
            .all(&self.db)
            .await?;
        let mut step_seen = HashSet::new();
        let mut step_ids = Vec::new();
        for goal_model in goals {
            if step_seen.insert(goal_model.step_id) {
                step_ids.push(goal_model.step_id);
            }
        }
        if step_ids.is_empty() {
            return Ok(Vec::new());
        }
        let steps = step::Entity::find()
            .filter(step::Column::Id.is_in(step_ids))
            .all(&self.db)
            .await?;
        let mut plan_seen = HashSet::new();
        let mut plan_ids = Vec::new();
        for step_model in steps {
            if plan_seen.insert(step_model.plan_id) {
                plan_ids.push(step_model.plan_id);
            }
        }
        Ok(plan_ids)
    }

    pub async fn comment_plans(&self, entries: Vec<(i64, String)>) -> Result<Vec<i64>, AppError> {
        let entries = normalize_comment_entries(entries);
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<i64> = entries.iter().map(|(id, _)| *id).collect();
        let txn = self.db.begin().await?;
        let result: Result<Vec<i64>, AppError> = async {
            let plans = plan::Entity::find()
                .filter(plan::Column::Id.is_in(ids.clone()))
                .all(&txn)
                .await?;
            let existing: HashSet<i64> = plans.iter().map(|plan| plan.id).collect();
            let missing: Vec<i64> = ids
                .iter()
                .cloned()
                .filter(|id| !existing.contains(id))
                .collect();
            if !missing.is_empty() {
                return Err(AppError::NotFound(format!(
                    "plan id(s) not found: {}",
                    join_ids(&missing)
                )));
            }

            let now = Utc::now();
            for (plan_id, comment) in entries {
                let mut active = plan::ActiveModel {
                    id: Set(plan_id),
                    ..Default::default()
                };
                active.comment = Set(Some(comment));
                active.last_session_id = Set(Some(self.session_id.clone()));
                active.updated_at = Set(now);
                active.update(&txn).await?;
            }

            Ok(ids)
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn comment_steps(&self, entries: Vec<(i64, String)>) -> Result<Vec<i64>, AppError> {
        let entries = normalize_comment_entries(entries);
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<i64> = entries.iter().map(|(id, _)| *id).collect();
        let txn = self.db.begin().await?;
        let result: Result<Vec<i64>, AppError> = async {
            let steps = step::Entity::find()
                .filter(step::Column::Id.is_in(ids.clone()))
                .all(&txn)
                .await?;
            let existing: HashSet<i64> = steps.iter().map(|step| step.id).collect();
            let missing: Vec<i64> = ids
                .iter()
                .cloned()
                .filter(|id| !existing.contains(id))
                .collect();
            if !missing.is_empty() {
                return Err(AppError::NotFound(format!(
                    "step id(s) not found: {}",
                    join_ids(&missing)
                )));
            }

            let mut seen = HashSet::new();
            let mut plan_ids = Vec::new();
            for step_model in &steps {
                if seen.insert(step_model.plan_id) {
                    plan_ids.push(step_model.plan_id);
                }
            }

            let now = Utc::now();
            for (step_id, comment) in entries {
                let mut active = step::ActiveModel {
                    id: Set(step_id),
                    ..Default::default()
                };
                active.comment = Set(Some(comment));
                active.updated_at = Set(now);
                active.update(&txn).await?;
            }

            if !plan_ids.is_empty() {
                self.touch_plans_with_conn(&txn, &plan_ids).await?;
            }

            Ok(plan_ids)
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn comment_goals(&self, entries: Vec<(i64, String)>) -> Result<Vec<i64>, AppError> {
        let entries = normalize_comment_entries(entries);
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<i64> = entries.iter().map(|(id, _)| *id).collect();
        let txn = self.db.begin().await?;
        let result: Result<Vec<i64>, AppError> = async {
            let goals = goal::Entity::find()
                .filter(goal::Column::Id.is_in(ids.clone()))
                .all(&txn)
                .await?;
            let existing: HashSet<i64> = goals.iter().map(|goal| goal.id).collect();
            let missing: Vec<i64> = ids
                .iter()
                .cloned()
                .filter(|id| !existing.contains(id))
                .collect();
            if !missing.is_empty() {
                return Err(AppError::NotFound(format!(
                    "goal id(s) not found: {}",
                    join_ids(&missing)
                )));
            }

            let mut seen = HashSet::new();
            let mut step_ids = Vec::new();
            for goal_model in &goals {
                if seen.insert(goal_model.step_id) {
                    step_ids.push(goal_model.step_id);
                }
            }

            let now = Utc::now();
            for (goal_id, comment) in entries {
                let mut active = goal::ActiveModel {
                    id: Set(goal_id),
                    ..Default::default()
                };
                active.comment = Set(Some(comment));
                active.updated_at = Set(now);
                active.update(&txn).await?;
            }

            let mut plan_ids = Vec::new();
            if !step_ids.is_empty() {
                let steps = step::Entity::find()
                    .filter(step::Column::Id.is_in(step_ids))
                    .all(&txn)
                    .await?;
                let mut seen = HashSet::new();
                for step_model in steps {
                    if seen.insert(step_model.plan_id) {
                        plan_ids.push(step_model.plan_id);
                    }
                }
            }

            if !plan_ids.is_empty() {
                self.touch_plans_with_conn(&txn, &plan_ids).await?;
            }

            Ok(plan_ids)
        }
        .await;

        finalize_transaction(txn, result).await
    }

    pub async fn update_goal(
        &self,
        id: i64,
        changes: GoalChanges,
    ) -> Result<(goal::Model, StatusChanges), AppError> {
        let txn = self.db.begin().await?;
        let result = self.update_goal_with_conn(&txn, id, changes).await;
        finalize_transaction(txn, result).await
    }

    pub async fn set_goal_status(
        &self,
        id: i64,
        status: GoalStatus,
    ) -> Result<(goal::Model, StatusChanges), AppError> {
        let changes = GoalChanges {
            status: Some(status),
            ..Default::default()
        };
        self.update_goal(id, changes).await
    }

    async fn update_goal_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        id: i64,
        changes: GoalChanges,
    ) -> Result<(goal::Model, StatusChanges), AppError> {
        if let Some(content) = changes.content.as_deref() {
            ensure_non_empty("goal content", content)?;
        }
        let mut active = goal::ActiveModel {
            id: Set(id),
            ..Default::default()
        };
        if let Some(content) = changes.content {
            active.content = Set(content);
        }
        if let Some(status) = changes.status {
            active.status = Set(status.as_str().to_string());
        }
        if let Some(comment) = changes.comment {
            active.comment = Set(Some(comment));
        }
        active.updated_at = Set(Utc::now());

        let model = match active.update(db).await {
            Ok(model) => model,
            Err(sea_orm::DbErr::RecordNotFound(_)) | Err(sea_orm::DbErr::RecordNotUpdated) => {
                return Err(AppError::NotFound(format!("goal id {id}")))
            }
            Err(err) => return Err(err.into()),
        };

        let changes = self
            .refresh_step_status_with_conn(db, model.step_id)
            .await?;
        if let Some(step_model) = step::Entity::find_by_id(model.step_id).one(db).await? {
            self.touch_plan_with_conn(db, step_model.plan_id).await?;
        }
        Ok((model, changes))
    }

    async fn set_goals_status_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        ids: &[i64],
        status: GoalStatus,
    ) -> Result<(u64, StatusChanges), AppError> {
        if ids.is_empty() {
            return Ok((0, StatusChanges::default()));
        }
        let unique_ids = unique_ids(ids);
        let goals = goal::Entity::find()
            .filter(goal::Column::Id.is_in(unique_ids.clone()))
            .all(db)
            .await?;
        let existing: HashSet<i64> = goals.iter().map(|goal| goal.id).collect();
        let missing: Vec<i64> = unique_ids
            .iter()
            .cloned()
            .filter(|id| !existing.contains(id))
            .collect();
        if !missing.is_empty() {
            return Err(AppError::NotFound(format!(
                "goal id(s) not found: {}",
                join_ids(&missing)
            )));
        }

        let now = Utc::now();
        let mut seen = HashSet::new();
        let mut step_ids = Vec::new();
        for goal_model in &goals {
            if seen.insert(goal_model.step_id) {
                step_ids.push(goal_model.step_id);
            }
            let mut active: goal::ActiveModel = goal_model.clone().into();
            active.status = Set(status.as_str().to_string());
            active.updated_at = Set(now);
            active.update(db).await?;
        }

        let mut changes = StatusChanges::default();
        for step_id in &step_ids {
            let updated = self.refresh_step_status_with_conn(db, *step_id).await?;
            changes.merge(updated);
        }

        let mut plan_ids = Vec::new();
        if !step_ids.is_empty() {
            let steps = step::Entity::find()
                .filter(step::Column::Id.is_in(step_ids))
                .all(db)
                .await?;
            let mut seen = HashSet::new();
            for step_model in steps {
                if seen.insert(step_model.plan_id) {
                    plan_ids.push(step_model.plan_id);
                }
            }
        }
        if !plan_ids.is_empty() {
            self.touch_plans_with_conn(db, &plan_ids).await?;
        }

        Ok((unique_ids.len() as u64, changes))
    }

    async fn set_all_goals_done_for_step_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        step_id: i64,
    ) -> Result<StatusChanges, AppError> {
        step::Entity::find_by_id(step_id)
            .one(db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("step id {step_id}")))?;
        let goals = self.goals_for_step_with_conn(db, step_id).await?;
        if goals.is_empty() {
            return Ok(StatusChanges::default());
        }
        let ids: Vec<i64> = goals.iter().map(|goal| goal.id).collect();
        let changes = self
            .set_goals_status_with_conn(db, &ids, GoalStatus::Done)
            .await?
            .1;
        Ok(changes)
    }

    pub async fn set_goals_status(
        &self,
        ids: &[i64],
        status: GoalStatus,
    ) -> Result<(u64, StatusChanges), AppError> {
        let txn = self.db.begin().await?;
        let result = self.set_goals_status_with_conn(&txn, ids, status).await;
        finalize_transaction(txn, result).await
    }

    pub async fn delete_goals(&self, ids: &[i64]) -> Result<(u64, StatusChanges), AppError> {
        let txn = self.db.begin().await?;
        let result: Result<(u64, StatusChanges), AppError> = async {
            if ids.is_empty() {
                return Ok((0, StatusChanges::default()));
            }
            let unique_ids = unique_ids(ids);
            let goals = goal::Entity::find()
                .filter(goal::Column::Id.is_in(unique_ids.clone()))
                .all(&txn)
                .await?;
            let existing: HashSet<i64> = goals.iter().map(|goal| goal.id).collect();
            let missing: Vec<i64> = unique_ids
                .iter()
                .cloned()
                .filter(|id| !existing.contains(id))
                .collect();
            if !missing.is_empty() {
                return Err(AppError::NotFound(format!(
                    "goal id(s) not found: {}",
                    join_ids(&missing)
                )));
            }
            let mut seen = HashSet::new();
            let mut step_ids = Vec::new();
            for goal in goals {
                if seen.insert(goal.step_id) {
                    step_ids.push(goal.step_id);
                }
            }

            let result = goal::Entity::delete_many()
                .filter(goal::Column::Id.is_in(unique_ids))
                .exec(&txn)
                .await?;

            let mut changes = StatusChanges::default();
            for step_id in &step_ids {
                let updated = self.refresh_step_status_with_conn(&txn, *step_id).await?;
                changes.merge(updated);
            }

            if !step_ids.is_empty() {
                let mut plan_ids = Vec::new();
                let steps = step::Entity::find()
                    .filter(step::Column::Id.is_in(step_ids))
                    .all(&txn)
                    .await?;
                let mut seen = HashSet::new();
                for step_model in steps {
                    if seen.insert(step_model.plan_id) {
                        plan_ids.push(step_model.plan_id);
                    }
                }
                if !plan_ids.is_empty() {
                    self.touch_plans_with_conn(&txn, &plan_ids).await?;
                }
            }

            Ok((result.rows_affected, changes))
        }
        .await;

        finalize_transaction(txn, result).await
    }

    async fn normalize_steps_for_plan<C: ConnectionTrait>(
        &self,
        db: &C,
        plan_id: i64,
    ) -> Result<(), AppError> {
        let mut steps = step::Entity::find()
            .filter(step::Column::PlanId.eq(plan_id))
            .order_by_asc(step::Column::SortOrder)
            .order_by_asc(step::Column::Id)
            .all(db)
            .await?;
        self.normalize_steps_in_place(&mut steps, db).await
    }

    async fn normalize_steps_in_place<C: ConnectionTrait>(
        &self,
        steps: &mut [step::Model],
        db: &C,
    ) -> Result<(), AppError> {
        let now = Utc::now();
        for (idx, step_model) in steps.iter_mut().enumerate() {
            let desired_order = (idx + 1) as i32;
            if step_model.sort_order != desired_order {
                let mut active: step::ActiveModel = step_model.clone().into();
                active.sort_order = Set(desired_order);
                active.updated_at = Set(now);
                active.update(db).await?;
                step_model.sort_order = desired_order;
                step_model.updated_at = now;
            }
        }
        Ok(())
    }
}

impl App {
    async fn touch_plan_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        plan_id: i64,
    ) -> Result<(), AppError> {
        let mut active = plan::ActiveModel {
            id: Set(plan_id),
            ..Default::default()
        };
        active.last_session_id = Set(Some(self.session_id.clone()));
        active.updated_at = Set(Utc::now());
        match active.update(db).await {
            Ok(_) => Ok(()),
            Err(sea_orm::DbErr::RecordNotFound(_)) | Err(sea_orm::DbErr::RecordNotUpdated) => {
                Err(AppError::NotFound(format!("plan id {plan_id}")))
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn touch_plans_with_conn<C: ConnectionTrait>(
        &self,
        db: &C,
        plan_ids: &[i64],
    ) -> Result<(), AppError> {
        for plan_id in plan_ids {
            self.touch_plan_with_conn(db, *plan_id).await?;
        }
        Ok(())
    }
}

async fn finalize_transaction<T>(
    txn: DatabaseTransaction,
    result: Result<T, AppError>,
) -> Result<T, AppError> {
    match result {
        Ok(value) => {
            txn.commit().await?;
            Ok(value)
        }
        Err(err) => {
            if let Err(rollback_err) = txn.rollback().await {
                return Err(rollback_err.into());
            }
            Err(err)
        }
    }
}

fn unique_ids(ids: &[i64]) -> Vec<i64> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for id in ids {
        if seen.insert(*id) {
            unique.push(*id);
        }
    }
    unique
}

fn normalize_comment_entries(entries: Vec<(i64, String)>) -> Vec<(i64, String)> {
    let mut seen: HashMap<i64, usize> = HashMap::new();
    let mut ordered: Vec<(i64, String)> = Vec::new();
    for (id, comment) in entries {
        if let Some(idx) = seen.get(&id) {
            ordered[*idx].1 = comment;
        } else {
            seen.insert(id, ordered.len());
            ordered.push((id, comment));
        }
    }
    ordered
}

fn join_ids(ids: &[i64]) -> String {
    ids.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn ensure_non_empty(label: &str, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::InvalidInput(format!("{label} cannot be empty")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::model::{
        GoalChanges, GoalStatus, PlanChanges, PlanInput, PlanStatus, StepChanges, StepExecutor,
        StepStatus,
    };
    use tempfile::TempDir;

    const TEST_CONVERSATION_ID: &str = "test-session";

    async fn setup_app() -> (TempDir, App) {
        let dir = TempDir::new().expect("temp dir");
        let db_path = db::resolve_db_path(dir.path());
        db::ensure_parent_dir(&db_path).expect("ensure parent");
        let db = db::connect(&db_path).await.expect("connect db");
        db::ensure_schema(&db).await.expect("ensure schema");
        (dir, App::new(db, TEST_CONVERSATION_ID.to_string()))
    }

    async fn create_plan(app: &App, title: &str) -> plan::Model {
        app.add_plan(PlanInput {
            title: title.to_string(),
            content: "Content".to_string(),
        })
        .await
        .expect("add plan")
    }

    async fn add_step(app: &App, plan_id: i64, content: &str, status: StepStatus) -> step::Model {
        let (steps, _) = app
            .add_steps_batch(
                plan_id,
                vec![content.to_string()],
                status,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");
        steps.into_iter().next().expect("step")
    }

    async fn add_goal(app: &App, step_id: i64, content: &str, status: GoalStatus) -> goal::Model {
        let (goals, _) = app
            .add_goals_batch(step_id, vec![content.to_string()], status)
            .await
            .expect("add goals");
        goals.into_iter().next().expect("goal")
    }

    #[tokio::test]
    async fn delete_plan_cascades_steps_and_goals() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["Step".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");
        let step_id = steps[0].id;
        app.add_goals_batch(step_id, vec!["Goal".to_string()], GoalStatus::Todo)
            .await
            .expect("add goals");

        app.delete_plan(plan.id).await.expect("delete plan");

        let step_count = step::Entity::find()
            .filter(step::Column::PlanId.eq(plan.id))
            .count(&app.db)
            .await
            .expect("count steps");
        assert_eq!(step_count, 0);
        let goal_count = goal::Entity::find()
            .count(&app.db)
            .await
            .expect("count goals");
        assert_eq!(goal_count, 0);
    }

    #[tokio::test]
    async fn delete_steps_errors_on_missing_ids() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["Step".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");
        let step_id = steps[0].id;

        let err = app.delete_steps(&[step_id, 9999]).await.unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("step id(s) not found"));
            }
            _ => panic!("unexpected error type"),
        }

        let step = app.get_step(step_id).await.expect("step still exists");
        assert_eq!(step.id, step_id);
    }

    #[tokio::test]
    async fn delete_goals_errors_on_missing_ids() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["Step".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");
        let step_id = steps[0].id;
        let (goals, _) = app
            .add_goals_batch(step_id, vec!["Goal".to_string()], GoalStatus::Todo)
            .await
            .expect("add goals");
        let goal_id = goals[0].id;

        let err = app.delete_goals(&[goal_id, 9999]).await.unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("goal id(s) not found"));
            }
            _ => panic!("unexpected error type"),
        }

        let goal = goal::Entity::find_by_id(goal_id)
            .one(&app.db)
            .await
            .expect("query goal")
            .expect("goal still exists");
        assert_eq!(goal.id, goal_id);
    }

    #[tokio::test]
    async fn deleting_last_goal_keeps_step_status() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["Step".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");
        let step_id = steps[0].id;
        let (goals, _) = app
            .add_goals_batch(step_id, vec!["Goal".to_string()], GoalStatus::Todo)
            .await
            .expect("add goals");
        let goal_id = goals[0].id;

        app.set_goal_status(goal_id, GoalStatus::Done)
            .await
            .expect("set goal done");
        let step = app.get_step(step_id).await.expect("get step");
        assert_eq!(step.status, StepStatus::Done.as_str());

        app.delete_goals(&[goal_id]).await.expect("delete goal");
        let step_after = app.get_step(step_id).await.expect("get step");
        assert_eq!(step_after.status, StepStatus::Done.as_str());
    }

    #[tokio::test]
    async fn update_plan_rejects_done_with_pending_step() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        let err = app
            .update_plan_with_active_clear(
                plan.id,
                PlanChanges {
                    status: Some(PlanStatus::Done),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("cannot mark plan done"));
                assert!(message.contains("next pending step"));
            }
            _ => panic!("unexpected error type"),
        }

        let plan_after = app.get_plan(plan.id).await.expect("get plan");
        assert_eq!(plan_after.status, PlanStatus::Todo.as_str());
    }

    #[tokio::test]
    async fn update_step_rejects_done_with_pending_goal() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;
        add_goal(&app, step.id, "Goal 1", GoalStatus::Todo).await;

        let err = app
            .update_step(
                step.id,
                StepChanges {
                    status: Some(StepStatus::Done),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("cannot mark step done"));
                assert!(message.contains("next pending goal"));
            }
            _ => panic!("unexpected error type"),
        }

        let step_after = app.get_step(step.id).await.expect("get step");
        assert_eq!(step_after.status, StepStatus::Todo.as_str());
    }

    #[tokio::test]
    async fn goal_completion_updates_step_plan_and_clears_active_plan() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;
        let goal = add_goal(&app, step.id, "Goal 1", GoalStatus::Todo).await;

        app.set_active_plan(plan.id, false)
            .await
            .expect("set active");

        let (_goal, changes) = app
            .set_goal_status(goal.id, GoalStatus::Done)
            .await
            .expect("set goal done");
        let step_after = app.get_step(step.id).await.expect("get step");
        let plan_after = app.get_plan(plan.id).await.expect("get plan");
        let active = app.get_active_plan().await.expect("get active");

        assert_eq!(step_after.status, StepStatus::Done.as_str());
        assert_eq!(plan_after.status, PlanStatus::Done.as_str());
        assert!(active.is_none());
        assert!(!changes.steps.is_empty());
        assert!(!changes.plans.is_empty());
        assert!(!changes.active_plans_cleared.is_empty());
    }

    #[tokio::test]
    async fn adding_goal_to_done_step_reopens_step_and_plan() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        app.update_step(
            step.id,
            StepChanges {
                status: Some(StepStatus::Done),
                ..Default::default()
            },
        )
        .await
        .expect("set step done");
        let plan_done = app.get_plan(plan.id).await.expect("get plan");
        assert_eq!(plan_done.status, PlanStatus::Done.as_str());

        let (_goals, changes) = app
            .add_goals_batch(step.id, vec!["Goal 1".to_string()], GoalStatus::Todo)
            .await
            .expect("add goals");

        let step_after = app.get_step(step.id).await.expect("get step");
        let plan_after = app.get_plan(plan.id).await.expect("get plan");
        assert_eq!(step_after.status, StepStatus::Todo.as_str());
        assert_eq!(plan_after.status, PlanStatus::Todo.as_str());
        assert!(!changes.steps.is_empty());
        assert!(!changes.plans.is_empty());
    }

    #[tokio::test]
    async fn add_steps_batch_inserts_at_position_and_shifts() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        app.add_steps_batch(
            plan.id,
            vec!["A".to_string(), "B".to_string(), "C".to_string()],
            StepStatus::Todo,
            StepExecutor::Ai,
            None,
        )
        .await
        .expect("add steps");

        app.add_steps_batch(
            plan.id,
            vec!["X".to_string(), "Y".to_string()],
            StepStatus::Todo,
            StepExecutor::Ai,
            Some(2),
        )
        .await
        .expect("add steps at");

        let (_plan, steps) = app.plan_with_steps(plan.id).await.expect("plan steps");
        let contents: Vec<_> = steps.iter().map(|step| step.content.as_str()).collect();
        let orders: Vec<_> = steps.iter().map(|step| step.sort_order).collect();
        assert_eq!(contents, vec!["A", "X", "Y", "B", "C"]);
        assert_eq!(orders, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn move_step_reorders_bounds() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["A".to_string(), "B".to_string(), "C".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");
        let id_a = steps[0].id;
        let id_c = steps[2].id;

        let moved = app.move_step(id_c, 1).await.expect("move step");
        let contents: Vec<_> = moved.iter().map(|step| step.content.as_str()).collect();
        assert_eq!(contents, vec!["C", "A", "B"]);

        let moved_again = app.move_step(id_c, 99).await.expect("move step end");
        let contents: Vec<_> = moved_again
            .iter()
            .map(|step| step.content.as_str())
            .collect();
        assert_eq!(contents, vec!["A", "B", "C"]);

        let final_step = app.get_step(id_a).await.expect("get step");
        assert_eq!(final_step.sort_order, 1);
    }

    #[tokio::test]
    async fn delete_goals_updates_step_status_when_remaining_done() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        add_goal(&app, step.id, "Done", GoalStatus::Done).await;
        let todo_goal = add_goal(&app, step.id, "Todo", GoalStatus::Todo).await;

        let step_before = app.get_step(step.id).await.expect("get step");
        assert_eq!(step_before.status, StepStatus::Todo.as_str());

        let (_deleted, changes) = app
            .delete_goals(&[todo_goal.id])
            .await
            .expect("delete goal");

        let step_after = app.get_step(step.id).await.expect("get step");
        let plan_after = app.get_plan(plan.id).await.expect("get plan");
        assert_eq!(step_after.status, StepStatus::Done.as_str());
        assert_eq!(plan_after.status, PlanStatus::Done.as_str());
        assert!(!changes.steps.is_empty());
        assert!(!changes.plans.is_empty());
    }

    #[tokio::test]
    async fn delete_steps_updates_plan_status_when_remaining_done() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["Done".to_string(), "Todo".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");

        app.update_step(
            steps[0].id,
            StepChanges {
                status: Some(StepStatus::Done),
                ..Default::default()
            },
        )
        .await
        .expect("set step done");

        let (_deleted, changes) = app.delete_steps(&[steps[1].id]).await.expect("delete step");

        let plan_after = app.get_plan(plan.id).await.expect("get plan");
        assert_eq!(plan_after.status, PlanStatus::Done.as_str());
        assert!(!changes.plans.is_empty());
    }

    #[tokio::test]
    async fn delete_steps_reorders_remaining() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["A".to_string(), "B".to_string(), "C".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");

        app.delete_steps(&[steps[1].id]).await.expect("delete step");

        let (_plan, remaining) = app.plan_with_steps(plan.id).await.expect("plan steps");
        let contents: Vec<_> = remaining.iter().map(|step| step.content.as_str()).collect();
        let orders: Vec<_> = remaining.iter().map(|step| step.sort_order).collect();
        assert_eq!(contents, vec!["A", "C"]);
        assert_eq!(orders, vec![1, 2]);
    }

    #[tokio::test]
    async fn delete_plan_clears_active_plan() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        app.set_active_plan(plan.id, false)
            .await
            .expect("set active");

        app.delete_plan(plan.id).await.expect("delete plan");

        let active = app.get_active_plan().await.expect("get active");
        assert!(active.is_none());
    }

    #[tokio::test]
    async fn active_plan_is_scoped_to_session() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = db::resolve_db_path(dir.path());
        db::ensure_parent_dir(&db_path).expect("ensure parent");

        let db_a = db::connect(&db_path).await.expect("connect db a");
        db::ensure_schema(&db_a).await.expect("ensure schema a");
        let db_b = db::connect(&db_path).await.expect("connect db b");
        db::ensure_schema(&db_b).await.expect("ensure schema b");

        let app_a = App::new(db_a, "session-a".to_string());
        let app_b = App::new(db_b, "session-b".to_string());

        let plan_a = create_plan(&app_a, "Plan A").await;
        let plan_b = create_plan(&app_a, "Plan B").await;

        app_a
            .set_active_plan(plan_a.id, false)
            .await
            .expect("set active a");
        app_b
            .set_active_plan(plan_b.id, false)
            .await
            .expect("set active b");

        let active_a = app_a.get_active_plan().await.expect("get active a");
        let active_b = app_b.get_active_plan().await.expect("get active b");

        assert_eq!(active_a.expect("active a").plan_id, plan_a.id);
        assert_eq!(active_b.expect("active b").plan_id, plan_b.id);
    }

    #[tokio::test]
    async fn active_plan_is_unique_per_plan() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = db::resolve_db_path(dir.path());
        db::ensure_parent_dir(&db_path).expect("ensure parent");

        let db_a = db::connect(&db_path).await.expect("connect db a");
        db::ensure_schema(&db_a).await.expect("ensure schema a");
        let db_b = db::connect(&db_path).await.expect("connect db b");
        db::ensure_schema(&db_b).await.expect("ensure schema b");

        let app_a = App::new(db_a, "session-a".to_string());
        let app_b = App::new(db_b, "session-b".to_string());

        let plan = create_plan(&app_a, "Plan A").await;

        app_a
            .set_active_plan(plan.id, false)
            .await
            .expect("set active a");
        let err = app_b.set_active_plan(plan.id, false).await.unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("already active in session"));
                assert!(message.contains("session-a"));
            }
            _ => panic!("unexpected error type"),
        }

        let active_a = app_a.get_active_plan().await.expect("get active a");
        let active_b = app_b.get_active_plan().await.expect("get active b");

        assert_eq!(active_a.expect("active a").plan_id, plan.id);
        assert!(active_b.is_none());
    }

    #[tokio::test]
    async fn active_plan_takeover_reassigns_plan() {
        let dir = TempDir::new().expect("temp dir");
        let db_path = db::resolve_db_path(dir.path());
        db::ensure_parent_dir(&db_path).expect("ensure parent");

        let db_a = db::connect(&db_path).await.expect("connect db a");
        db::ensure_schema(&db_a).await.expect("ensure schema a");
        let db_b = db::connect(&db_path).await.expect("connect db b");
        db::ensure_schema(&db_b).await.expect("ensure schema b");

        let app_a = App::new(db_a, "session-a".to_string());
        let app_b = App::new(db_b, "session-b".to_string());

        let plan = create_plan(&app_a, "Plan A").await;

        app_a
            .set_active_plan(plan.id, false)
            .await
            .expect("set active a");
        app_b
            .set_active_plan(plan.id, true)
            .await
            .expect("set active b");

        let active_a = app_a.get_active_plan().await.expect("get active a");
        let active_b = app_b.get_active_plan().await.expect("get active b");

        assert!(active_a.is_none());
        assert_eq!(active_b.expect("active b").plan_id, plan.id);
    }

    #[tokio::test]
    async fn list_steps_missing_plan_errors() {
        let (_dir, app) = setup_app().await;
        let query = StepQuery::default();
        let err = app.list_steps_filtered(9999, &query).await.unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("plan id 9999"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn count_steps_missing_plan_errors() {
        let (_dir, app) = setup_app().await;
        let query = StepQuery::default();
        let err = app.count_steps(9999, &query).await.unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("plan id 9999"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn list_goals_missing_step_errors() {
        let (_dir, app) = setup_app().await;
        let query = GoalQuery::default();
        let err = app.list_goals_filtered(9999, &query).await.unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("step id 9999"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn count_goals_missing_step_errors() {
        let (_dir, app) = setup_app().await;
        let query = GoalQuery::default();
        let err = app.count_goals(9999, &query).await.unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("step id 9999"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn add_steps_batch_empty_contents_returns_empty() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        let (steps, changes) = app
            .add_steps_batch(
                plan.id,
                Vec::new(),
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");

        assert!(steps.is_empty());
        assert!(changes.is_empty());
        let plan_after = app.get_plan(plan.id).await.expect("get plan");
        assert_eq!(plan_after.status, PlanStatus::Todo.as_str());
    }

    #[tokio::test]
    async fn add_goals_batch_empty_contents_returns_empty() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        let (goals, changes) = app
            .add_goals_batch(step.id, Vec::new(), GoalStatus::Todo)
            .await
            .expect("add goals");

        assert!(goals.is_empty());
        assert!(changes.is_empty());
        let step_after = app.get_step(step.id).await.expect("get step");
        assert_eq!(step_after.status, StepStatus::Todo.as_str());
    }

    #[tokio::test]
    async fn update_plan_reports_missing_id() {
        let (_dir, app) = setup_app().await;
        let err = app
            .update_plan_with_active_clear(9999, PlanChanges::default())
            .await
            .unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("plan id 9999"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_step_reports_missing_id() {
        let (_dir, app) = setup_app().await;
        let err = app
            .update_step(9999, StepChanges::default())
            .await
            .unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("step id 9999"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_goal_reports_missing_id() {
        let (_dir, app) = setup_app().await;
        let err = app
            .update_goal(9999, GoalChanges::default())
            .await
            .unwrap_err();
        match err {
            AppError::NotFound(message) => {
                assert!(message.contains("goal id 9999"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn delete_steps_deduplicates_ids() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        let (steps, _) = app
            .add_steps_batch(
                plan.id,
                vec!["A".to_string(), "B".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .expect("add steps");

        let ids = vec![steps[0].id, steps[0].id, steps[1].id];
        let (deleted, _) = app.delete_steps(&ids).await.expect("delete steps");
        assert_eq!(deleted, 2);

        let remaining = step::Entity::find()
            .filter(step::Column::PlanId.eq(plan.id))
            .count(&app.db)
            .await
            .expect("count steps");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn delete_goals_deduplicates_ids() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        let (goals, _) = app
            .add_goals_batch(
                step.id,
                vec!["G1".to_string(), "G2".to_string()],
                GoalStatus::Todo,
            )
            .await
            .expect("add goals");

        let ids = vec![goals[0].id, goals[0].id, goals[1].id];
        let (deleted, _) = app.delete_goals(&ids).await.expect("delete goals");
        assert_eq!(deleted, 2);

        let remaining = goal::Entity::find()
            .filter(goal::Column::StepId.eq(step.id))
            .count(&app.db)
            .await
            .expect("count goals");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn update_plan_all_steps_done_allows_done() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        app.add_steps_batch(
            plan.id,
            vec!["A".to_string(), "B".to_string()],
            StepStatus::Done,
            StepExecutor::Ai,
            None,
        )
        .await
        .expect("add steps");

        let (updated, _cleared) = app
            .update_plan_with_active_clear(
                plan.id,
                PlanChanges {
                    status: Some(PlanStatus::Done),
                    ..Default::default()
                },
            )
            .await
            .expect("update plan");
        assert_eq!(updated.status, PlanStatus::Done.as_str());
    }

    #[tokio::test]
    async fn update_step_all_goals_done_allows_done() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        app.add_goals_batch(
            step.id,
            vec!["G1".to_string(), "G2".to_string()],
            GoalStatus::Done,
        )
        .await
        .expect("add goals");

        let (updated, _changes) = app
            .update_step(
                step.id,
                StepChanges {
                    status: Some(StepStatus::Done),
                    ..Default::default()
                },
            )
            .await
            .expect("update step");
        assert_eq!(updated.status, StepStatus::Done.as_str());
    }

    #[tokio::test]
    async fn add_plan_rejects_empty_title() {
        let (_dir, app) = setup_app().await;
        let err = app
            .add_plan(PlanInput {
                title: "   ".to_string(),
                content: "Content".to_string(),
            })
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("plan title cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn add_plan_rejects_empty_content() {
        let (_dir, app) = setup_app().await;
        let err = app
            .add_plan(PlanInput {
                title: "Title".to_string(),
                content: "   ".to_string(),
            })
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("plan content cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn update_plan_rejects_empty_title() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let err = app
            .update_plan_with_active_clear(
                plan.id,
                PlanChanges {
                    title: Some("   ".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("plan title cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn update_plan_rejects_empty_content() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let err = app
            .update_plan_with_active_clear(
                plan.id,
                PlanChanges {
                    content: Some("   ".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("plan content cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }
    }

    #[tokio::test]
    async fn add_steps_batch_rejects_empty_content() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;

        let err = app
            .add_steps_batch(
                plan.id,
                vec!["   ".to_string()],
                StepStatus::Todo,
                StepExecutor::Ai,
                None,
            )
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("step content cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }

        let remaining = step::Entity::find()
            .filter(step::Column::PlanId.eq(plan.id))
            .count(&app.db)
            .await
            .expect("count steps");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn add_goals_batch_rejects_empty_content() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        let err = app
            .add_goals_batch(step.id, vec!["   ".to_string()], GoalStatus::Todo)
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("goal content cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }

        let remaining = goal::Entity::find()
            .filter(goal::Column::StepId.eq(step.id))
            .count(&app.db)
            .await
            .expect("count goals");
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn update_step_rejects_empty_content() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;

        let err = app
            .update_step(
                step.id,
                StepChanges {
                    content: Some("   ".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("step content cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }

        let step_after = app.get_step(step.id).await.expect("get step");
        assert_eq!(step_after.content, "Step 1");
    }

    #[tokio::test]
    async fn update_goal_rejects_empty_content() {
        let (_dir, app) = setup_app().await;
        let plan = create_plan(&app, "Plan").await;
        let step = add_step(&app, plan.id, "Step 1", StepStatus::Todo).await;
        let goal = add_goal(&app, step.id, "Goal 1", GoalStatus::Todo).await;

        let err = app
            .update_goal(
                goal.id,
                GoalChanges {
                    content: Some("   ".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        match err {
            AppError::InvalidInput(message) => {
                assert!(message.contains("goal content cannot be empty"));
            }
            _ => panic!("unexpected error type"),
        }

        let goal_after = goal::Entity::find_by_id(goal.id)
            .one(&app.db)
            .await
            .expect("query goal")
            .expect("goal exists");
        assert_eq!(goal_after.content, "Goal 1");
    }
}
