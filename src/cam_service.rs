use anyhow::{Context, Result};
use regex::Regex;
use crate::db::db::{DashcamDb };
use crate::db::db_worker::{DBMessage,DBWorker,start_db_worker};
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tracing::{error, info};

use crate::config::AppConfig;
use crate::recording_pipeline::RecordingPipeline;
use crate::recording_pipeline_factory::build_pipelines_from_config;

pub struct CamService {
    pub pipelines: Vec<Arc<Mutex<RecordingPipeline>>>,
    pub running: Arc<AtomicBool>,
    pub db_worker_handle: Option<JoinHandle<()>>,
    pub db_sender: Arc<Sender<DBMessage>>,
    pub app_config: AppConfig
}

impl CamService {
    /// Construct CamService from AppConfig:
    /// - start DB worker thread
    /// - build one RecordingPipeline per enabled camera via factory
    pub fn new(cfg: AppConfig) -> Result<Self> {
        info!("Creating CamService");

        info!("Creating DB Worker...");
        let (dbsender, dbrecvr) = channel::<DBMessage>();
        let db_worker = DBWorker::new(dbrecvr, &cfg)?;
        let dbhandle = start_db_worker(db_worker);
        let dbsender = Arc::new(dbsender);

        info!("Building pipelines from AppConfig via factory...");
        let pipeline_vec = build_pipelines_from_config(&cfg, dbsender.clone()).with_context(|| {
            "CamService: build_pipelines_from_config() failed"
        })?;
        let pipelines: Vec<Arc<Mutex<RecordingPipeline>>> =
            pipeline_vec.into_iter().map(|p| Arc::new(Mutex::new(p))).collect();

        let service = CamService {
            pipelines,
            running: Arc::new(AtomicBool::new(false)),
            db_worker_handle: Some(dbhandle),
            db_sender: dbsender,
            app_config: cfg
        };

        service.prep_dir_for_service()?;

        Ok(service)
    }

    /// Start all pipelines (each pipeline spawns its own thread).
    pub fn main_loop(&mut self) -> Result<()> {
        info!(
            "Starting CamService::main_loop() at {}",
            chrono::Local::now().format("%m-%d-%Y %H:%M:%S")
        );

        self.running.store(true, Ordering::SeqCst);

        for (idx, pipeline_arc) in self.pipelines.iter().enumerate() {
            let mut pipeline = pipeline_arc.lock().unwrap();
            if pipeline.is_running() {
                info!("Pipeline #{} already running, skipping start", idx);
                continue;
            }
            info!("Starting pipeline #{}", idx);
            if let Err(e) = pipeline.start_pipeline() {
                error!("Failed to start pipeline #{}: {:#}", idx, e);
            }
        }

        Ok(())
    }

    /// Stop all pipelines.
    pub fn kill_main_loop(&mut self) -> Result<()> {
        info!("Killing CamService main loop");
        self.running.store(false, Ordering::SeqCst);

        for (idx, pipeline_arc) in self.pipelines.iter().enumerate() {
            let mut pipeline = pipeline_arc.lock().unwrap();
            if pipeline.is_running() {
                info!("Stopping pipeline #{}", idx);
                if let Err(e) = pipeline.stop_pipeline() {
                    error!("Error stopping pipeline #{}: {:#}", idx, e);
                }
            }
        }

        info!(
            "Killed CamService at {}",
            chrono::Local::now().format("%m-%d-%Y %H:%M:%S")
        );
        Ok(())
    }

    fn prep_dir_for_service(&self) -> Result<()> {
        // Create directories
        fs::create_dir_all(&self.app_config.global.recording_root)?;

        // Delete any segment*.ts or livestream.m3u8
        let segment_regex = Regex::new(r"segment\d*\.ts")?;

        for entry in fs::read_dir(&self.app_config.global.recording_root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                for dir in fs::read_dir(entry.path())? {
                    let dir = dir?;
                    let filename = dir.file_name();
                    let filename_str = filename.to_string_lossy();

                    if dir.file_type()?.is_file()
                        && (filename_str.contains("livestream.m3u8")
                            || segment_regex.is_match(&filename_str))
                    {
                        fs::remove_file(dir.path())?;
                    }
                }
            }
        }

        Ok(())
    }
}

impl Drop for CamService {
    fn drop(&mut self) {
        info!("Dropping CamService");
        let _ = self.kill_main_loop();
    }
}
