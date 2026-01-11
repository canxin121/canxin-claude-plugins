use sea_orm::entity::prelude::*;

use super::step;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "goals")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub step_id: i64,
    pub content: String,
    pub status: String,
    pub comment: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    Step,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::Step => Entity::belongs_to(step::Entity)
                .from(Column::StepId)
                .to(step::Column::Id)
                .into(),
        }
    }
}

impl Related<step::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Step.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
