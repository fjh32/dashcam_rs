use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::config::{AppConfig, CameraConfig, GlobalConfig, SourceKind, SinkConfig, CameraRole};
use crate::db_worker::DBMessage;
use crate::recording_pipeline::{RecordingConfig, RecordingPipeline, PipelineSource, PipelineSink};
use crate::libcamera_pipeline_source::LibcameraPipelineSource;
use crate::v4l2_pipeline_source::V4l2PipelineSource;
use crate::ts_file_pipeline_sink::TsFilePipelineSink;
use crate::hls_pipeline_sink::HlsPipelineSink;
use crate::constants::*;

/// Build a RecordingConfig for a specific camera.
///
/// - recording_dir: global.recording_root / camera.key
/// - video_*: from global if set, otherwise from constants.
fn build_recording_config(global: &GlobalConfig, cam: &CameraConfig) -> RecordingConfig {
    // Base from Default/Constants, then override
    let mut cfg = RecordingConfig::default();

    // put recordings per-camera under recording_root/key
    let mut dir = PathBuf::from(&global.recording_root);
    dir.push(&cam.key);
    cfg.recording_dir = dir.to_string_lossy().to_string();

    if let Some(w) = global.video_width {
        cfg.video_width = w as i32;
    }
    if let Some(h) = global.video_height {
        cfg.video_height = h as i32;
    }
    if let Some(fps) = global.video_framerate {
        cfg.frame_rate = fps as i32;
    }

    // segment duration comes from sinks:
    // Pick dashcam_ts duration if present, else NvrTs, else Hls, else default.
    if let Some(dash_ts) = cam.sinks.iter().find_map(|s| {
        if let SinkConfig::DashcamTs { segment_duration_sec, .. } = s {
            Some(*segment_duration_sec)
        } else {
            None
        }
    }) {
        cfg.video_duration = dash_ts;
    } else if let Some(nvr_ts) = cam.sinks.iter().find_map(|s| {
        if let SinkConfig::NvrTs { segment_duration_sec } = s {
            Some(*segment_duration_sec)
        } else {
            None
        }
    }) {
        cfg.video_duration = nvr_ts;
    } else if let Some(hls) = cam.sinks.iter().find_map(|s| {
        if let SinkConfig::Hls { segment_duration_sec } = s {
            Some(*segment_duration_sec)
        } else {
            None
        }
    }) {
        cfg.video_duration = hls;
    } else {
        // fallback to whatever RecordingConfig::default() gave us
        // cfg.video_duration already set
    }

    cfg
}

/// Build a PipelineSource from a camera's source config.
fn build_source_for_camera(
    cam: &CameraConfig,
    rec_cfg: &RecordingConfig,
) -> Result<Box<dyn PipelineSource>> {
    match cam.source.kind {
        SourceKind::Libcamera => {
            Ok(Box::new(LibcameraPipelineSource::new(rec_cfg.clone())))
        }
        SourceKind::V4l2 => {
            Ok(Box::new(V4l2PipelineSource::new(rec_cfg.clone())))
        }
        SourceKind::Rtsp => {
            // TODO: implement an RtspPipelineSource
            Err(anyhow!("Rtsp source not implemented yet"))
        }
    }
}

/// Build the sinks for a camera based on its sink configs.
fn build_sinks_for_camera(
    cam: &CameraConfig,
    rec_cfg: &RecordingConfig,
    db_sender: Sender<DBMessage>,
) -> Result<Vec<Box<dyn PipelineSink>>> {
    let mut sinks: Vec<Box<dyn PipelineSink>> = Vec::new();

    for sink_cfg in &cam.sinks {
        match sink_cfg {
            SinkConfig::DashcamTs {
                max_segments,
                segment_duration_sec: _,
            } => {
                // For now, TsFilePipelineSink already uses RecordingConfig.video_duration
                // for segment duration, and we pass per-sink ring size here.
                let ts_sink = TsFilePipelineSink::new_with_existing_dbworker(
                    rec_cfg.clone(),
                    *max_segments,
                    db_sender.clone().into(),
                )?;
                sinks.push(Box::new(ts_sink) as Box<dyn PipelineSink>);
            }

            SinkConfig::Hls { segment_duration_sec: _ } => {
                // Your HlsPipelineSink::new currently just takes RecordingConfig.
                // If you later want different HLS segment durations per sink,
                // you can add that parameter to the sink and pass segment_seconds here.
                let hls_sink = HlsPipelineSink::new(rec_cfg.clone());
                sinks.push(Box::new(hls_sink) as Box<dyn PipelineSink>);
            }

            SinkConfig::NvrTs { segment_duration_sec: _ } => {
                // Placeholder for future NVR TS sink implementation.
                // For now we can either ignore it or return an error.
                // Let's return an error so we don't silently ignore config.
                return Err(anyhow!("NvrTs sink not implemented yet"));
            }
        }
    }

    if sinks.is_empty() {
        return Err(anyhow!(
            "Camera '{}' has no usable sinks configured",
            cam.key
        ));
    }

    Ok(sinks)
}

/// Build a single RecordingPipeline for a camera.
pub fn build_pipeline_for_camera(
    global: &GlobalConfig,
    cam: &CameraConfig,
    db_sender: Sender<DBMessage>,
) -> Result<RecordingPipeline> {
    if !cam.enabled {
        return Err(anyhow!("Camera '{}' is disabled", cam.key));
    }

    // For now, only support certain roles in this build
    match cam.role {
        CameraRole::Dashcam | CameraRole::Nvr | CameraRole::Preview => {
            // all allowed for now; you can restrict if needed
        }
    }

    let rec_cfg = build_recording_config(global, cam);

    // Create the RecordingPipeline
    let mut pipeline = RecordingPipeline::new(rec_cfg.clone())?;

    // Source
    let source = build_source_for_camera(cam, &rec_cfg)?;
    pipeline.set_source(source);

    // Sinks
    let sinks = build_sinks_for_camera(cam, &rec_cfg, db_sender)?;
    for sink in sinks {
        pipeline.add_sink(sink);
    }

    Ok(pipeline)
}

/// Helper: build all pipelines for all enabled cameras in AppConfig.
pub fn build_pipelines_from_config(
    cfg: &AppConfig,
    db_sender: Sender<DBMessage>,
) -> Result<Vec<RecordingPipeline>> {
    let mut pipelines = Vec::new();

    for cam in &cfg.cameras {
        if !cam.enabled {
            continue;
        }
        let p = build_pipeline_for_camera(&cfg.global, cam, db_sender.clone())?;
        pipelines.push(p);
    }

    Ok(pipelines)
}
