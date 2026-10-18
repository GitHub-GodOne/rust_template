use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "tls_certificates")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub domain: String,
    #[sea_orm(column_type = "Text")]
    pub alt_names: String,
    pub status: String,
    pub auto_renew: bool,
    pub issuer: Option<String>,
    pub not_before: Option<DateTimeWithTimeZone>,
    pub not_after: Option<DateTimeWithTimeZone>,
    #[sea_orm(column_type = "Text", nullable)]
    pub cert_path: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub key_path: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub last_error: Option<String>,
    pub last_attempt_at: Option<DateTimeWithTimeZone>,
    pub renewed_at: Option<DateTimeWithTimeZone>,
    pub created_by: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::domain_mappings::Entity")]
    DomainMappings,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::CreatedBy",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    Users,
}

impl Related<super::domain_mappings::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::DomainMappings.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
