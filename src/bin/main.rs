use gpt_images::{app::App, data_manager, service_manager, setup, vnc_manager};
use loco_rs::cli;
use migration::Migrator;

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    if service_manager::handle_if_requested()? {
        return Ok(());
    }
    if vnc_manager::handle_if_requested()? {
        return Ok(());
    }
    if data_manager::handle_if_requested()? {
        return Ok(());
    }
    setup::run_if_required().await?;
    cli::main::<App, Migrator>().await
}
