use anyhow::Result;
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::info;

mod cam_service;
mod constants;
mod db;
mod db_worker;
mod hls_pipeline_sink;
mod libcamera_pipeline_source;
mod log;
mod recording_pipeline;
mod ts_file_pipeline_sink;
mod v4l2_pipeline_source;

use cam_service::CamService;

fn main() -> Result<()> {
    log::setup_trace_logging();

    let mut cam_service = CamService::new()?;

    let running = cam_service.running.clone();
    let mut signals = Signals::new(&[SIGINT, SIGTERM, SIGQUIT, SIGHUP])?;

    cam_service.main_loop()?;

    for sig in signals.forever() {
        info!("Exiting cleanly. Received signal {}", sig);
        // running.store(false, Ordering::SeqCst);
        cam_service.kill_main_loop()?;
        std::process::exit(sig);
    }
    

    Ok(())
}
