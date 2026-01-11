use sea_orm::entity::prelude::*;

use super::step;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "plans")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub title: String,
    pub content: String,
    pub status: String,
    pub comment: Option<String>,
    pub last_session_id: Option<String>,
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
            Self::Step => Entity::has_many(step::Entity).into(),
        }
    }
}

impl Related<step::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Step.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
