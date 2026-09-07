use gpt_images::{app::App, service_manager, setup};
use loco_rs::cli;
use migration::Migrator;

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    if service_manager::handle_if_requested()? {
        return Ok(());
    }
    setup::run_if_required().await?;
    cli::main::<App, Migrator>().await
}
