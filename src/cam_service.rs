use anyhow::{Result, Context};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufRead, BufReader, Write};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use regex::Regex;

use crate::recording_pipeline::{RecordingPipeline, RecordingConfig};
// use crate::v4l2_pipeline_source::V4l2PipelineSource;
use crate::libcamera_pipeline_source::LibcameraPipelineSource;
use crate::ts_file_pipeline_sink::TsFilePipelineSink;
use crate::hls_pipeline_sink::HlsPipelineSink;

const SOCKET_PATH: &str = "/tmp/dashcam.sock";

#[cfg(debug_assertions)]
const VIDEO_DURATION: u64 = 2;
#[cfg(debug_assertions)]
const RECORDING_DIR: &str = "./recordings/";
#[cfg(debug_assertions)]
const RECORDING_SAVE_DIR: &str = "./recordings/save/";
#[cfg(debug_assertions)]
const SEGMENTS_TO_KEEP: i32 = 86400 / 2 * 2; // 2 days worth

#[cfg(not(debug_assertions))]
const VIDEO_DURATION: u64 = 2;
#[cfg(not(debug_assertions))]
const RECORDING_DIR: &str = "/var/lib/dashcam/recordings/";
#[cfg(not(debug_assertions))]
const RECORDING_SAVE_DIR: &str = "/var/lib/dashcam/recordings/save/";
#[cfg(not(debug_assertions))]
const SEGMENTS_TO_KEEP: i32 = 86400 / 2 * 2; // 2 days worth

pub struct CamService {
    pub recording_pipeline: Arc<Mutex<RecordingPipeline>>,
    pub running: Arc<AtomicBool>,
    pub recording_dir: String,
    pub recording_save_dir: String,
}

impl CamService {
    pub fn new() -> Result<Self> {
        println!("Creating CamService");

        let recording_dir = RECORDING_DIR.to_string();
        let recording_save_dir = RECORDING_SAVE_DIR.to_string();

        let config = RecordingConfig {
            recording_dir: recording_dir.clone(),
            video_duration: VIDEO_DURATION,
            video_width: 640,
            video_height: 480,
            frame_rate: 10,
        };

        println!("Creating RecordingPipeline...");
        let pipeline = RecordingPipeline::new(config.clone())?;
        let pipeline_arc = Arc::new(Mutex::new(pipeline));

        println!("Creating sinks BEFORE locking pipeline...");
        
        #[cfg(not(feature = "rpi"))]
        let (source, ts_sink, hls_sink) = {
            use crate::v4l2_pipeline_source::V4l2PipelineSource;

            println!("V4L2 MODE CamService");
            let source = Box::new(V4l2PipelineSource::new());
            let ts_sink = Box::new(TsFilePipelineSink::new_with_max_segments(
                config.clone(),
                false,
                SEGMENTS_TO_KEEP,
            ));
            let hls_sink = Box::new(HlsPipelineSink::new(config.clone()));
            (source, ts_sink, hls_sink)
        };
        
        // #[cfg(feature = "rpi")]
        let (source, ts_sink, hls_sink) = {
            println!("RPI MODE CamService");
            let source = Box::new(LibcameraPipelineSource::new());
            let ts_sink = Box::new(TsFilePipelineSink::new_with_max_segments(
                config.clone(),
                false,
                SEGMENTS_TO_KEEP,
            ));
            let hls_sink = Box::new(HlsPipelineSink::new(config.clone()));
            (source, ts_sink, hls_sink)
        };

        println!("NOW locking pipeline to add source and sinks...");
        {
            let mut pipeline = pipeline_arc.lock().unwrap();
            pipeline.set_source(source);
            pipeline.add_sink(ts_sink);
            pipeline.add_sink(hls_sink);
        }
        println!("Pipeline configured!");

        let service = CamService {
            recording_pipeline: pipeline_arc,
            running: Arc::new(AtomicBool::new(false)),
            recording_dir,
            recording_save_dir,
        };

        service.prep_dir_for_service()?;
        // service.create_listening_socket()?;

        Ok(service)
    }

    pub fn main_loop(&mut self) -> Result<()> {
        println!(
            "Starting CamService::main_loop() at {}",
            chrono::Local::now().format("%m-%d-%Y %H:%M:%S")
        );

        self.running.store(true, Ordering::SeqCst);

        // Start pipeline (non-blocking)
        {
            let mut pipeline = self.recording_pipeline.lock().unwrap();
            pipeline.start_pipeline()?;
        }

        // Listen on socket (blocking)
        self.listen_on_socket()?;

        println!("Exiting main loop");
        Ok(())
    }

    pub fn kill_main_loop(&mut self) -> Result<()> {
        println!("Killing main loop");
        self.running.store(false, Ordering::SeqCst);

        {
            let mut pipeline = self.recording_pipeline.lock().unwrap();
            pipeline.stop_pipeline()?;
        }

        self.remove_listening_socket()?;

        println!(
            "Killed CamService at {}",
            chrono::Local::now().format("%m-%d-%Y %H:%M:%S")
        );
        Ok(())
    }

    fn save_recordings(&self, seconds_back_to_save: u64) -> Result<()> {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs() as i64;
        let threshold_time = current_time - seconds_back_to_save as i64;

        let segment_pattern = Regex::new(r"output_(\d+)\.ts")?;
        let subdir_pattern = Regex::new(r"^\d+$")?;

        let mut candidates = Vec::new();

        // Walk through subdirectories
        for dir_entry in fs::read_dir(&self.recording_dir)? {
            let dir_entry = dir_entry?;
            if !dir_entry.file_type()?.is_dir() {
                continue;
            }

            let subdir_name = dir_entry.file_name();
            let subdir_name_str = subdir_name.to_string_lossy();
            if !subdir_pattern.is_match(&subdir_name_str) {
                continue;
            }

            // Walk through files in subdirectory
            for file_entry in fs::read_dir(dir_entry.path())? {
                let file_entry = file_entry?;
                if !file_entry.file_type()?.is_file() {
                    continue;
                }

                let filename = file_entry.file_name();
                let filename_str = filename.to_string_lossy();
                if !segment_pattern.is_match(&filename_str) {
                    continue;
                }

                // Check modification time
                let metadata = file_entry.metadata()?;
                let mtime = metadata.modified()?
                    .duration_since(UNIX_EPOCH)?
                    .as_secs() as i64;

                if mtime > threshold_time {
                    candidates.push(file_entry.path());
                }
            }
        }

        // Trigger new video segment
        // Note: createNewVideo not implemented in RecordingPipeline yet
        // You'll need to add this method

        // Sort candidates by modification time (newest first)
        candidates.sort_by(|a, b| {
            let a_mtime = fs::metadata(a).and_then(|m| m.modified()).ok();
            let b_mtime = fs::metadata(b).and_then(|m| m.modified()).ok();
            b_mtime.cmp(&a_mtime)
        });

        // Create timestamped save directory
        let timestamp_dir = format!("{}{}/", self.recording_save_dir, current_time);
        fs::create_dir_all(&timestamp_dir)?;

        println!(
            "Saving {} recent segments to {}",
            candidates.len(),
            timestamp_dir
        );

        // Copy files
        for src_path in candidates {
            let filename = src_path.file_name()
                .context("Invalid filename")?;
            let dst_path = Path::new(&timestamp_dir).join(filename);

            match fs::copy(&src_path, &dst_path) {
                Ok(_) => println!("Saved file: {:?}", dst_path),
                Err(e) => eprintln!(
                    "Warning: Failed to copy file from {:?} to {:?}: {}",
                    src_path, dst_path, e
                ),
            }
        }

        Ok(())
    }


    fn remove_listening_socket(&self) -> Result<()> {
        fs::remove_file(SOCKET_PATH)
            .context("Failed to remove Unix socket")?;
        println!("Unix socket removed");
        Ok(())
    }

    fn listen_on_socket(&self) -> Result<()> {
        println!("Starting listen_on_socket()");

        // Remove old socket if exists
        let _ = fs::remove_file(SOCKET_PATH);

        let listener = UnixListener::bind(SOCKET_PATH)
            .context("Failed to bind to Unix socket")?;

        println!("Unix socket created at {}", SOCKET_PATH);

        listener.set_nonblocking(true)?;

        let save_regex = Regex::new(r"save:(\d+)")?;

        while self.running.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _addr)) => {
                    println!("Got connection on socket");
                    if let Err(e) = self.handle_connection(stream, &save_regex) {
                        eprintln!("Error handling connection: {}", e);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                Err(e) => {
                    eprintln!("Error accepting connection: {}", e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
            }
        }

        println!("Closing listen_on_socket()");
        Ok(())
    }

    fn handle_connection(&self, stream: UnixStream, save_regex: &Regex) -> Result<()> {
        let mut reader = BufReader::new(&stream);
        let mut message = String::new();

        reader.read_line(&mut message)?;
        let message = message.trim();

        println!("Received message on socket: {}", message);

        if message == "kill" {
            // Note: This is tricky - we're in a borrowed context
            // Better to send a response and let main loop handle it
            // For now, just set running to false
            self.running.store(false, Ordering::SeqCst);
        } else if let Some(captures) = save_regex.captures(message) {
            if let Some(seconds_str) = captures.get(1) {
                if let Ok(seconds) = seconds_str.as_str().parse::<u64>() {
                    self.save_recordings(seconds)?;
                }
            }
        }

        // Optionally send a response back
        // let mut writer = stream;
        // writer.write_all(b"OK\n")?;

        Ok(())
    }

    fn prep_dir_for_service(&self) -> Result<()> {
        // Create directories
        fs::create_dir_all(&self.recording_dir)?;
        fs::create_dir_all(&self.recording_save_dir)?;

        // Delete any segment*.ts or livestream.m3u8
        let segment_regex = Regex::new(r"segment\d*\.ts")?;

        for entry in fs::read_dir(&self.recording_dir)? {
            let entry = entry?;
            let filename = entry.file_name();
            let filename_str = filename.to_string_lossy();

            if entry.file_type()?.is_file()
                && (filename_str.contains("livestream.m3u8")
                    || segment_regex.is_match(&filename_str))
            {
                fs::remove_file(entry.path())?;
            }
        }

        Ok(())
    }
}

impl Drop for CamService {
    fn drop(&mut self) {
        println!("Dropping CamService");
        let _ = self.kill_main_loop();
    }
}