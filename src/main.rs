use anyhow::{Context, Result};
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::fs;
use tracing::info;

mod cam_service;
mod constants;
mod config;
mod utils;
mod db;
mod db_worker;
mod hls_pipeline_sink;
mod libcamera_pipeline_source;
mod log;
mod recording_pipeline;
mod ts_file_pipeline_sink;
mod v4l2_pipeline_source;

mod recording_pipeline_factory;

use cam_service::CamService;
use config::AppConfig;

pub const CONFIG_PATH: &str = "/var/lib/dashcam/config.toml";

fn load_app_config() -> Result<AppConfig> {
    // You can change this path or make it an env var if you like
    let path = CONFIG_PATH;

    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file at '{}'", path))?;

    let cfg: AppConfig = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse TOML config at '{}'", path))?;

    Ok(cfg)
}

fn main() -> Result<()> {
    log::setup_trace_logging();

    let cfg = load_app_config()?;

    let mut cam_service = CamService::new(&cfg)?;

    let running = cam_service.running.clone();
    let mut signals = Signals::new(&[SIGINT, SIGTERM, SIGQUIT, SIGHUP])?;

    cam_service.main_loop()?;

    for sig in signals.forever() {
        info!("Exiting cleanly. Received signal {}", sig);
        running.store(false, std::sync::atomic::Ordering::SeqCst);
        cam_service.kill_main_loop()?;
        std::process::exit(sig);
    }

    #[allow(unreachable_code)]
    Ok(())
}
