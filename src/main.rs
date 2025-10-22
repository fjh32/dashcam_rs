use anyhow::Result;
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

mod recording_pipeline;
// mod v4l2_pipeline_source;
mod libcamera_pipeline_source;
mod ts_file_pipeline_sink;
mod hls_pipeline_sink;
mod cam_service;

use cam_service::CamService;

fn main() -> Result<()> {
    // Create CamService
    let mut cam_service = CamService::new()?;
    
    // Setup signal handlers - just set the running flag, don't try to lock!
    let running = cam_service.running.clone();
    let mut signals = Signals::new(&[SIGINT, SIGTERM, SIGQUIT, SIGHUP])?;
    
    thread::spawn(move || {
        for sig in signals.forever() {
            println!("Exiting cleanly. Received signal {}", sig);
            running.store(false, Ordering::SeqCst);  // Just set flag, don't lock!
            std::process::exit(sig);
        }
    });

    // Run main loop
    cam_service.main_loop()?;

    Ok(())
}