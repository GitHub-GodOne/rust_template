use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "host_temperatures")]
pub struct Model {
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    #[sea_orm(primary_key)]
    pub id: i32,
    pub hostname: String,
    pub captured_at: DateTimeWithTimeZone,
    pub max_temperature_celsius: f32,
    pub peak_sensor: String,
    pub sensor_count: i32,
    pub sensors_json: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
