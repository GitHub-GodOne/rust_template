use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "project_cli_run_logs")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub seq: i32,
    pub stream: String,
    #[sea_orm(column_type = "Text")]
    pub text: String,
    pub run_id: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::project_cli_runs::Entity",
        from = "Column::RunId",
        to = "super::project_cli_runs::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    ProjectCliRuns,
}

impl Related<super::project_cli_runs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProjectCliRuns.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
