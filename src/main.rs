use anyhow::Result;
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

mod recording_pipeline;
mod v4l2_pipeline_source;
mod libcamera_pipeline_source;
mod ts_file_pipeline_sink;
mod hls_pipeline_sink;
mod cam_service;


use cam_service::CamService;

fn main() -> Result<()> {
    // Create CamService
    let mut cam_service = CamService::new()?;
    
    let running = cam_service.running.clone();
    let mut signals = Signals::new(&[SIGINT, SIGTERM, SIGQUIT, SIGHUP])?;
    
    thread::spawn(move || {
        for sig in signals.forever() {
            println!("Exiting cleanly. Received signal {}", sig);
            running.store(false, Ordering::SeqCst);
            std::process::exit(sig);
        }
    });

    cam_service.main_loop()?;

    Ok(())
}