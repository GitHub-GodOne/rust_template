use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, Set,
};
use serde::{Deserialize, Serialize};
use sysinfo::{Components, System};

use crate::models::_entities::host_temperatures;

const COLLECTION_INTERVAL: Duration = Duration::from_secs(15);
// Keep compatibility with stable Rust versions before duration_constructors.
#[allow(clippy::duration_suboptimal_units)]
const RECORD_INTERVAL: Duration = Duration::from_secs(60);
pub const RETENTION_DAYS: i64 = 7;
static CURRENT: OnceLock<Mutex<TemperatureSnapshot>> = OnceLock::new();

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct TemperatureReading {
    pub sensor: String,
    pub temperature_celsius: Option<f32>,
    pub critical_celsius: Option<f32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, utoipa::ToSchema)]
pub struct TemperatureSnapshot {
    pub captured_at: Option<String>,
    pub sensors: Vec<TemperatureReading>,
    pub available: bool,
    pub message: Option<String>,
}

pub fn current_snapshot() -> TemperatureSnapshot {
    CURRENT
        .get_or_init(|| {
            Mutex::new(TemperatureSnapshot {
                message: Some("正在读取主机温度传感器".to_string()),
                ..Default::default()
            })
        })
        .lock()
        .map_or_else(
            |_| TemperatureSnapshot::default(),
            |snapshot| snapshot.clone(),
        )
}

pub fn start_collector(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut last_recorded: Option<Instant> = None;
        loop {
            match tokio::task::spawn_blocking(collect_snapshot).await {
                Ok(snapshot) => {
                    if let Ok(mut cached) = CURRENT
                        .get_or_init(|| Mutex::new(TemperatureSnapshot::default()))
                        .lock()
                    {
                        *cached = snapshot.clone();
                    }
                    if last_recorded.is_none_or(|time| time.elapsed() >= RECORD_INTERVAL) {
                        match record_snapshot(&db, &snapshot).await {
                            Ok(_) => {
                                last_recorded = Some(Instant::now());
                                if let Err(error) = prune_history(&db, Utc::now()).await {
                                    tracing::warn!(%error, "failed to prune temperature history");
                                }
                            }
                            Err(error) => {
                                tracing::warn!(%error, "failed to record host temperature");
                            }
                        }
                    }
                }
                Err(error) => tracing::warn!(%error, "temperature collector failed"),
            }
            tokio::time::sleep(COLLECTION_INTERVAL).await;
        }
    });
}

fn valid_temperature(value: Option<f32>) -> Option<f32> {
    value.filter(|value| value.is_finite() && *value >= -273.15)
}

#[must_use]
pub fn collect_snapshot() -> TemperatureSnapshot {
    let components = Components::new_with_refreshed_list();
    let mut sensors: BTreeMap<String, TemperatureReading> = BTreeMap::new();
    for component in &components {
        let reading = TemperatureReading {
            sensor: component.label().to_string(),
            temperature_celsius: valid_temperature(component.temperature()),
            critical_celsius: valid_temperature(component.critical()),
        };
        // Repeated labels represent the same displayed sensor; keep the highest readable value.
        let entry = sensors
            .entry(reading.sensor.clone())
            .or_insert_with(|| reading.clone());
        if reading.temperature_celsius > entry.temperature_celsius {
            *entry = reading;
        }
    }
    let sensors = sensors.into_values().collect::<Vec<_>>();
    let available = sensors
        .iter()
        .any(|sensor| sensor.temperature_celsius.is_some());
    TemperatureSnapshot {
        captured_at: Some(Utc::now().to_rfc3339()),
        sensors,
        available,
        message: (!available)
            .then(|| "未检测到可读取的温度传感器，请检查主机支持及运行账户权限".to_string()),
    }
}

/// Persists a sample only when it contains a usable temperature reading.
///
/// # Errors
/// Returns a database error or an error for a missing capture timestamp.
pub async fn record_snapshot(
    db: &DatabaseConnection,
    snapshot: &TemperatureSnapshot,
) -> Result<Option<host_temperatures::Model>, DbErr> {
    let Some((peak_sensor, maximum)) = snapshot
        .sensors
        .iter()
        .filter_map(|sensor| {
            valid_temperature(sensor.temperature_celsius).map(|value| (&sensor.sensor, value))
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
    else {
        return Ok(None);
    };
    let captured_at = snapshot
        .captured_at
        .as_deref()
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .ok_or_else(|| DbErr::Custom("temperature sample has no valid capture time".to_string()))?;
    let sensor_count = snapshot
        .sensors
        .iter()
        .filter(|sensor| valid_temperature(sensor.temperature_celsius).is_some())
        .count();
    let sensors_json = serde_json::to_string(&snapshot.sensors)
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    host_temperatures::ActiveModel {
        hostname: Set(System::host_name().unwrap_or_else(|| "unknown".to_string())),
        captured_at: Set(captured_at),
        max_temperature_celsius: Set(maximum),
        peak_sensor: Set(peak_sensor.clone()),
        sensor_count: Set(i32::try_from(sensor_count).unwrap_or(i32::MAX)),
        sensors_json: Set(sensors_json),
        ..Default::default()
    }
    .insert(db)
    .await
    .map(Some)
}

/// Removes temperature records older than the retention window.
///
/// # Errors
/// Returns any database deletion error.
pub async fn prune_history(
    db: &DatabaseConnection,
    now: chrono::DateTime<Utc>,
) -> Result<(), DbErr> {
    host_temperatures::Entity::delete_many()
        .filter(
            host_temperatures::Column::CapturedAt.lt(now - chrono::Duration::days(RETENTION_DAYS)),
        )
        .exec(db)
        .await?;
    Ok(())
}

#[cfg(test)]
pub(crate) async fn test_database() -> DatabaseConnection {
    use migration::{Migrator, MigratorTrait, SchemaManager};
    let db = sea_orm::Database::connect("sqlite::memory:").await.unwrap();
    let migration = Migrator::migrations()
        .into_iter()
        .find(|migration| migration.name() == "m20261007_000024_host_temperatures")
        .unwrap();
    migration.up(&SchemaManager::new(&db)).await.unwrap();
    db
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{PaginatorTrait, QueryOrder};

    #[test]
    fn invalid_readings_are_missing_and_zero_celsius_is_valid() {
        assert_eq!(valid_temperature(Some(0.0)), Some(0.0));
        assert_eq!(valid_temperature(Some(-20.0)), Some(-20.0));
        assert_eq!(valid_temperature(Some(f32::NAN)), None);
        assert_eq!(valid_temperature(Some(f32::INFINITY)), None);
        assert_eq!(valid_temperature(Some(-300.0)), None);
        assert_eq!(valid_temperature(None), None);
    }

    #[tokio::test]
    async fn unavailable_sensors_do_not_create_a_fake_record() {
        let db = test_database().await;
        let snapshot = TemperatureSnapshot {
            sensors: vec![TemperatureReading {
                sensor: "missing".to_string(),
                temperature_celsius: None,
                critical_celsius: None,
            }],
            ..Default::default()
        };
        assert!(record_snapshot(&db, &snapshot).await.unwrap().is_none());
        assert_eq!(
            host_temperatures::Entity::find().count(&db).await.unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn persists_sensor_details_and_prunes_only_expired_samples() {
        let db = test_database().await;
        let now = Utc::now();
        let mut snapshot = TemperatureSnapshot {
            captured_at: Some((now - chrono::Duration::days(8)).to_rfc3339()),
            sensors: vec![
                TemperatureReading {
                    sensor: "CPU".to_string(),
                    temperature_celsius: Some(45.5),
                    critical_celsius: Some(95.0),
                },
                TemperatureReading {
                    sensor: "Board".to_string(),
                    temperature_celsius: Some(51.2),
                    critical_celsius: None,
                },
                TemperatureReading {
                    sensor: "Unknown".to_string(),
                    temperature_celsius: None,
                    critical_celsius: None,
                },
            ],
            available: true,
            message: None,
        };
        let old = record_snapshot(&db, &snapshot).await.unwrap().unwrap();
        assert_eq!(old.max_temperature_celsius, 51.2);
        assert_eq!(old.peak_sensor, "Board");
        assert_eq!(old.sensor_count, 2);
        let details: Vec<TemperatureReading> = serde_json::from_str(&old.sensors_json).unwrap();
        assert_eq!(details.len(), 3);
        assert_eq!(details[0].critical_celsius, Some(95.0));
        assert_eq!(details[2].temperature_celsius, None);
        snapshot.captured_at = Some(now.to_rfc3339());
        let current = record_snapshot(&db, &snapshot).await.unwrap().unwrap();
        prune_history(&db, now).await.unwrap();
        let remaining = host_temperatures::Entity::find()
            .order_by_asc(host_temperatures::Column::Id)
            .all(&db)
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, current.id);
    }
}
