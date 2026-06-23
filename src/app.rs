use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Hooks, Initializer},
    bgworker::{BackgroundWorker, Queue},
    boot::{create_app, BootResult, StartMode},
    config::Config,
    controller::{middleware as loco_middleware, AppRoutes},
    db::{self, truncate_table},
    environment::Environment,
    task::Tasks,
    Result,
};
use migration::Migrator;
use std::path::Path;

#[allow(unused_imports)]
use crate::{
    controllers,
    models::_entities::{
        ai_image_generations, command_run_logs, command_runs, command_templates,
        command_workflow_run_steps, command_workflow_runs, command_workflow_steps,
        command_workflows, content_articles, content_categories, data_scopes, database_backups,
        database_restores, departments, dict_items, dict_types, email_templates, menus,
        operation_logs, payment_callbacks, payment_channels, payment_orders, payment_refunds,
        permissions, rate_limit_events, rate_limit_rules, refresh_tokens, role_data_scopes,
        role_menus, role_permissions, roles, scheduled_task_runs, scheduled_tasks, storage_buckets,
        storage_profiles, system_notifications, system_settings, tenants, upload_files,
        upload_tasks, user_departments, user_roles, users, work_order_assignments,
        work_order_attachments, work_order_comments, work_orders,
    },
    tasks,
    workers::downloader::DownloadWorker,
};

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(
        mode: StartMode,
        environment: &Environment,
        config: Config,
    ) -> Result<BootResult> {
        create_app::<Self, Migrator>(mode, environment, config).await
    }

    async fn initializers(_ctx: &AppContext) -> Result<Vec<Box<dyn Initializer>>> {
        Ok(vec![])
    }

    fn routes(_ctx: &AppContext) -> AppRoutes {
        AppRoutes::with_default_routes() // controller routes below
            .add_route(controllers::auth::routes())
            .add_route(controllers::admin::routes())
            .add_route(controllers::docs::routes())
            .add_route(controllers::frontend::routes())
    }

    fn middlewares(ctx: &AppContext) -> Vec<Box<dyn loco_middleware::MiddlewareLayer>> {
        let mut stack = loco_middleware::default_middleware_stack(ctx);
        stack.push(Box::new(
            crate::middleware::rate_limit::RateLimitMiddleware::new(ctx),
        ));
        stack
    }

    async fn connect_workers(ctx: &AppContext, queue: &Queue) -> Result<()> {
        queue.register(DownloadWorker::build(ctx)).await?;
        Ok(())
    }

    #[allow(unused_variables)]
    fn register_tasks(tasks: &mut Tasks) {
        tasks.register(crate::tasks::admin::RecoverAdmin);
        tasks.register(crate::tasks::admin::SyncFixtureBaseline);
        tasks.register(crate::tasks::operations::RunDueScheduledTasks);
        // tasks-inject (do not remove)
    }
    async fn truncate(ctx: &AppContext) -> Result<()> {
        truncate_table(&ctx.db, content_articles::Entity).await?;
        truncate_table(&ctx.db, content_categories::Entity).await?;
        truncate_table(&ctx.db, payment_callbacks::Entity).await?;
        truncate_table(&ctx.db, payment_refunds::Entity).await?;
        truncate_table(&ctx.db, payment_orders::Entity).await?;
        truncate_table(&ctx.db, payment_channels::Entity).await?;
        truncate_table(&ctx.db, work_order_attachments::Entity).await?;
        truncate_table(&ctx.db, work_order_assignments::Entity).await?;
        truncate_table(&ctx.db, work_order_comments::Entity).await?;
        truncate_table(&ctx.db, work_orders::Entity).await?;
        truncate_table(&ctx.db, rate_limit_events::Entity).await?;
        truncate_table(&ctx.db, rate_limit_rules::Entity).await?;
        truncate_table(&ctx.db, database_restores::Entity).await?;
        truncate_table(&ctx.db, database_backups::Entity).await?;
        truncate_table(&ctx.db, command_workflow_run_steps::Entity).await?;
        truncate_table(&ctx.db, command_workflow_runs::Entity).await?;
        truncate_table(&ctx.db, command_workflow_steps::Entity).await?;
        truncate_table(&ctx.db, command_run_logs::Entity).await?;
        truncate_table(&ctx.db, command_runs::Entity).await?;
        truncate_table(&ctx.db, command_workflows::Entity).await?;
        truncate_table(&ctx.db, command_templates::Entity).await?;
        truncate_table(&ctx.db, scheduled_task_runs::Entity).await?;
        truncate_table(&ctx.db, scheduled_tasks::Entity).await?;
        truncate_table(&ctx.db, system_notifications::Entity).await?;
        truncate_table(&ctx.db, ai_image_generations::Entity).await?;
        truncate_table(&ctx.db, upload_tasks::Entity).await?;
        truncate_table(&ctx.db, upload_files::Entity).await?;
        truncate_table(&ctx.db, storage_buckets::Entity).await?;
        truncate_table(&ctx.db, storage_profiles::Entity).await?;
        truncate_table(&ctx.db, dict_items::Entity).await?;
        truncate_table(&ctx.db, dict_types::Entity).await?;
        truncate_table(&ctx.db, system_settings::Entity).await?;
        truncate_table(&ctx.db, email_templates::Entity).await?;
        truncate_table(&ctx.db, operation_logs::Entity).await?;
        truncate_table(&ctx.db, role_data_scopes::Entity).await?;
        truncate_table(&ctx.db, role_menus::Entity).await?;
        truncate_table(&ctx.db, role_permissions::Entity).await?;
        truncate_table(&ctx.db, user_departments::Entity).await?;
        truncate_table(&ctx.db, user_roles::Entity).await?;
        truncate_table(&ctx.db, data_scopes::Entity).await?;
        truncate_table(&ctx.db, menus::Entity).await?;
        truncate_table(&ctx.db, permissions::Entity).await?;
        truncate_table(&ctx.db, roles::Entity).await?;
        truncate_table(&ctx.db, refresh_tokens::Entity).await?;
        truncate_table(&ctx.db, users::Entity).await?;
        truncate_table(&ctx.db, departments::Entity).await?;
        truncate_table(&ctx.db, tenants::Entity).await?;
        Ok(())
    }
    async fn seed(ctx: &AppContext, base: &Path) -> Result<()> {
        db::seed::<tenants::ActiveModel>(&ctx.db, &base.join("tenants.yaml").display().to_string())
            .await?;
        db::seed::<users::ActiveModel>(&ctx.db, &base.join("users.yaml").display().to_string())
            .await?;
        db::seed::<roles::ActiveModel>(&ctx.db, &base.join("roles.yaml").display().to_string())
            .await?;
        db::seed::<permissions::ActiveModel>(
            &ctx.db,
            &base.join("permissions.yaml").display().to_string(),
        )
        .await?;
        db::seed::<menus::ActiveModel>(&ctx.db, &base.join("menus.yaml").display().to_string())
            .await?;
        db::seed::<system_settings::ActiveModel>(
            &ctx.db,
            &base.join("system_settings.yaml").display().to_string(),
        )
        .await?;
        db::seed::<email_templates::ActiveModel>(
            &ctx.db,
            &base.join("email_templates.yaml").display().to_string(),
        )
        .await?;
        db::seed::<system_notifications::ActiveModel>(
            &ctx.db,
            &base.join("system_notifications.yaml").display().to_string(),
        )
        .await?;
        db::seed::<scheduled_tasks::ActiveModel>(
            &ctx.db,
            &base.join("scheduled_tasks.yaml").display().to_string(),
        )
        .await?;
        db::seed::<rate_limit_rules::ActiveModel>(
            &ctx.db,
            &base.join("rate_limit_rules.yaml").display().to_string(),
        )
        .await?;
        db::seed::<storage_profiles::ActiveModel>(
            &ctx.db,
            &base.join("storage_profiles.yaml").display().to_string(),
        )
        .await?;
        db::seed::<storage_buckets::ActiveModel>(
            &ctx.db,
            &base.join("storage_buckets.yaml").display().to_string(),
        )
        .await?;
        db::seed::<dict_types::ActiveModel>(
            &ctx.db,
            &base.join("dict_types.yaml").display().to_string(),
        )
        .await?;
        db::seed::<dict_items::ActiveModel>(
            &ctx.db,
            &base.join("dict_items.yaml").display().to_string(),
        )
        .await?;
        db::seed::<data_scopes::ActiveModel>(
            &ctx.db,
            &base.join("data_scopes.yaml").display().to_string(),
        )
        .await?;
        db::seed::<departments::ActiveModel>(
            &ctx.db,
            &base.join("departments.yaml").display().to_string(),
        )
        .await?;
        db::seed::<user_roles::ActiveModel>(
            &ctx.db,
            &base.join("user_roles.yaml").display().to_string(),
        )
        .await?;
        db::seed::<role_permissions::ActiveModel>(
            &ctx.db,
            &base.join("role_permissions.yaml").display().to_string(),
        )
        .await?;
        db::seed::<role_menus::ActiveModel>(
            &ctx.db,
            &base.join("role_menus.yaml").display().to_string(),
        )
        .await?;
        db::seed::<role_data_scopes::ActiveModel>(
            &ctx.db,
            &base.join("role_data_scopes.yaml").display().to_string(),
        )
        .await?;
        db::seed::<user_departments::ActiveModel>(
            &ctx.db,
            &base.join("user_departments.yaml").display().to_string(),
        )
        .await?;
        Ok(())
    }
}
