use async_trait::async_trait;
use chrono::offset::Local;
use loco_rs::{
    app::AppContext,
    task::{Task, TaskInfo, Vars},
    Error, Result,
};
use sea_orm::{ColumnTrait, Condition, EntityTrait, QueryFilter};

use crate::{controllers::admin::scheduled_tasks::run_task, models::_entities::scheduled_tasks};

pub struct RunDueScheduledTasks;

#[async_trait]
impl Task for RunDueScheduledTasks {
    fn task(&self) -> TaskInfo {
        TaskInfo {
            name: "operations.run_due_scheduled_tasks".to_string(),
            detail: "Run enabled scheduled tasks whose next_run_at is due".to_string(),
        }
    }

    async fn run(&self, ctx: &AppContext, _vars: &Vars) -> Result<()> {
        let now = Local::now();
        let tasks = scheduled_tasks::Entity::find()
            .filter(scheduled_tasks::Column::Enabled.eq(true))
            .filter(
                Condition::any()
                    .add(scheduled_tasks::Column::NextRunAt.is_null())
                    .add(scheduled_tasks::Column::NextRunAt.lte(now)),
            )
            .all(&ctx.db)
            .await
            .map_err(|err| Error::Message(err.to_string()))?;

        for task in tasks {
            if let Err(error) = run_task(ctx, &task, None, "scheduled").await {
                tracing::error!(task_id = task.id, error = ?error, "scheduled task failed");
            }
        }

        Ok(())
    }
}
