use gpt_images::{
    app::App, data_manager, rdp_manager, service_manager, services::ssh_session_host, setup,
    vnc_manager,
};
use loco_rs::cli;
use migration::Migrator;

#[tokio::main]
async fn main() -> loco_rs::Result<()> {
    // Internal: the detached process that keeps one SSH session's shell.
    if ssh_session_host::run_if_requested() {
        return Ok(());
    }
    if service_manager::handle_if_requested()? {
        return Ok(());
    }
    if vnc_manager::handle_if_requested()? {
        return Ok(());
    }
    if rdp_manager::handle_if_requested().await? {
        return Ok(());
    }
    if data_manager::handle_if_requested()? {
        return Ok(());
    }
    setup::run_if_required().await?;
    cli::main::<App, Migrator>().await
}
