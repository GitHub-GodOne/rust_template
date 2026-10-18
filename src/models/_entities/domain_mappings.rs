use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "domain_mappings")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    #[sea_orm(unique)]
    pub domain: String,
    pub mode: String,
    pub upstream_host: String,
    pub upstream_port: i32,
    pub public_port: i32,
    pub ipv4_mode: String,
    pub ipv4_address: Option<String>,
    pub ipv6_mode: String,
    pub ipv6_address: Option<String>,
    pub enabled: bool,
    pub status: String,
    #[sea_orm(column_type = "Text", nullable)]
    pub last_error: Option<String>,
    pub synced_at: Option<DateTimeWithTimeZone>,
    pub created_by: Option<i32>,
    pub certificate_id: Option<i32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::tls_certificates::Entity",
        from = "Column::CertificateId",
        to = "super::tls_certificates::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    TlsCertificates,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::CreatedBy",
        to = "super::users::Column::Id",
        on_update = "NoAction",
        on_delete = "SetNull"
    )]
    Users,
}

impl Related<super::tls_certificates::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TlsCertificates.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
