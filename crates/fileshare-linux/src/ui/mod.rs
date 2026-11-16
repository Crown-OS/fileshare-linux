//! The window.

mod benchmark;
mod overview;
mod pages;
mod root;
mod sparkline;
mod state;
#[cfg(test)]
mod tests;
mod text;
mod worker;

use anyhow::Context;
use crownui::AppConfig;
use crownui::kit::{INTER_FAMILY, inter_font};

const INITIAL_SIZE: (u32, u32) = (980, 700);
const MIN_SIZE: (u32, u32) = (720, 520);

pub(crate) fn run() -> anyhow::Result<()> {
    let rig_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("cannot start the rig runtime")?;
    let config = AppConfig::new("FileShare")
        .app_id("fileshare-linux")
        .size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .min_size(MIN_SIZE.0, MIN_SIZE.1)
        .font(inter_font().to_vec(), Some(INTER_FAMILY));
    crownui::run(root::root(rig_runtime.handle().clone()), config).context("the window failed")?;
    Ok(())
}
