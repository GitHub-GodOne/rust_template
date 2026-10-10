use std::{future::Future, time::Duration};

use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

const CHECK_INTERVAL: Duration = Duration::from_secs(5);
const CHECK_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_CONSECUTIVE_FAILURES: u8 = 3;

/// Starts the database health guard used by the installed system service.
///
/// Checks the application pool, including acquisition and the query itself.
/// After sustained failures, exits so the service manager can create a fresh
/// pool. Transient failures recover without restarting the application.
pub fn start_service_watchdog(database: DatabaseConnection) {
    if std::env::var_os("GPT_IMAGES_DB_WATCHDOG").is_none() {
        return;
    }

    tokio::spawn(async move {
        let mut failures = 0;
        tracing::info!("database watchdog started (5s interval, 3s timeout, 3 failures)");

        loop {
            // Sleep after each check so a slow query never causes catch-up bursts.
            tokio::time::sleep(CHECK_INTERVAL).await;
            let result = bounded_check(
                database.query_one(Statement::from_string(
                    database.get_database_backend(),
                    "SELECT 1".to_owned(),
                )),
                CHECK_TIMEOUT,
            )
            .await;
            match result {
                Ok(Ok(Some(_))) => failures = 0,
                failure => {
                    failures += 1;
                    tracing::warn!(
                        consecutive_failures = failures,
                        result = ?failure,
                        "database health check failed"
                    );
                    if should_restart(failures) {
                        tracing::error!(
                            consecutive_failures = failures,
                            "database stayed unavailable; exiting for launchd to recreate the connection pool"
                        );
                        std::process::exit(75);
                    }
                }
            }
        }
    });
}

async fn bounded_check<T>(
    check: impl Future<Output = T>,
    deadline: Duration,
) -> Result<T, tokio::time::error::Elapsed> {
    tokio::time::timeout(deadline, check).await
}

const fn should_restart(consecutive_failures: u8) -> bool {
    consecutive_failures >= MAX_CONSECUTIVE_FAILURES
}

#[cfg(test)]
mod tests {
    use super::{bounded_check, should_restart, MAX_CONSECUTIVE_FAILURES};

    #[tokio::test]
    async fn hung_query_cannot_block_the_watchdog() {
        let result = bounded_check(
            std::future::pending::<()>(),
            std::time::Duration::from_millis(10),
        )
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn waits_for_the_configured_failure_threshold() {
        assert!(!should_restart(MAX_CONSECUTIVE_FAILURES - 1));
        assert!(should_restart(MAX_CONSECUTIVE_FAILURES));
    }
}
