use sea_orm::entity::prelude::*;

use super::{goal, plan};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "steps")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub plan_id: i64,
    pub content: String,
    pub status: String,
    pub executor: String,
    pub sort_order: i32,
    pub comment: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Plan,
    Goal,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Plan => Entity::belongs_to(plan::Entity)
                .from(Column::PlanId)
                .to(plan::Column::Id)
                .into(),
            Self::Goal => Entity::has_many(goal::Entity).into(),
        }
    }
}

impl Related<plan::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Plan.def()
    }
}

impl Related<goal::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Goal.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
