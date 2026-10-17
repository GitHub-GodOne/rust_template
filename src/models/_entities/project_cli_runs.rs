use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "project_cli_runs")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(column_type = "Text")]
    pub path: String,
    #[sea_orm(column_type = "Text")]
    pub args: String,
    #[sea_orm(column_type = "Text")]
    pub command_line: String,
    pub environment: String,
    pub status: String,
    pub exit_code: Option<i32>,
    #[sea_orm(column_type = "Text", nullable)]
    pub error: Option<String>,
    pub started_at: DateTimeWithTimeZone,
    pub finished_at: Option<DateTimeWithTimeZone>,
    pub duration_ms: Option<i64>,
    pub created_by_name: String,
    pub last_seq: i32,
    pub output_bytes: i64,
    pub truncated: bool,
    pub created_by: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::project_cli_run_logs::Entity")]
    ProjectCliRunLogs,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::CreatedBy",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    Users,
}

impl Related<super::project_cli_run_logs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ProjectCliRunLogs.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
