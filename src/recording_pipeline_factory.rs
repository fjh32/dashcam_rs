use anyhow::{anyhow, Result};
use crate::db::db::{DashcamDb };
use crate::db::db_worker::{DBMessage,DBWorker,start_db_worker};
use crate::pipeline_sinks::hls_pipeline_sink::HlsPipelineSink;
use crate::pipeline_sinks::ts_file_pipeline_sink::TsFilePipelineSink;
use crate::pipeline_sinks::pipeline_sink::PipelineSink;
use crate::pipeline_sources::v4l2_pipeline_source::V4l2PipelineSource;
use crate::pipeline_sources::libcamera_pipeline_source::LibcameraPipelineSource;
use crate::pipeline_sources::pipeline_source::PipelineSource;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::sync::mpsc::Sender;

use crate::config::{AppConfig, CameraConfig, GlobalConfig, SourceKind, SinkConfig, CameraRole};
use crate::recording_pipeline::{RecordingConfig, RecordingPipeline};


fn get_camera_id_for_camera(
    cam: &CameraConfig,
    db_sender: &Arc<Sender<DBMessage>>,
) -> Result<i64> {
    let (tx, rx) = mpsc::channel();

    db_sender.send(DBMessage::GetCameraIdByKey {
        camera_key: cam.key.clone(),
        reply: tx,
    })?;

    match rx.recv() {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(anyhow!(
            "DBWorker could not find camera_id for key '{}'",
            cam.key
        )),
        Err(e) => Err(anyhow!(
            "DBWorker channel closed while waiting for camera_id for key '{}': {}",
            cam.key,
            e
        )),
    }
}


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

    if let Some(w) = cam.video_width {
        cfg.video_width = w as i32;
    }
    if let Some(h) = cam.video_height {
        cfg.video_height = h as i32;
    }
    if let Some(fps) = cam.video_framerate {
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
        if let SinkConfig::NvrTs { segment_duration_sec, .. } = s {
            Some(*segment_duration_sec)
        } else {
            None
        }
    }) {
        cfg.video_duration = nvr_ts;
    } else if let Some(hls) = cam.sinks.iter().find_map(|s| {
        if let SinkConfig::Hls { segment_duration_sec, .. } = s {
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

fn build_sinks_for_camera(
    cam: &CameraConfig,
    rec_cfg: &RecordingConfig,
    db_sender: Arc<Sender<DBMessage>>,
) -> Result<Vec<Box<dyn PipelineSink>>> {
    let mut sinks: Vec<Box<dyn PipelineSink>> = Vec::new();

    // Resolve camera_id once per camera through DBWorker
    let camera_id = get_camera_id_for_camera(cam, &db_sender)?;

    for sink_cfg in &cam.sinks {
        match sink_cfg {
            SinkConfig::DashcamTs {
                max_segments,
                segment_duration_sec: _,
                sink_id
            } => {
                // TsFilePipelineSink now needs camera_id and max_segments
                let ts_sink = TsFilePipelineSink::new(
                    rec_cfg.clone(),
                    camera_id,
                    *sink_id,
                    *max_segments,
                    db_sender.clone(),
                )?;
                sinks.push(Box::new(ts_sink) as Box<dyn PipelineSink>);
            }

            SinkConfig::Hls { segment_duration_sec: _ , sink_id: _} => {
                let hls_sink = HlsPipelineSink::new(rec_cfg.clone());
                sinks.push(Box::new(hls_sink) as Box<dyn PipelineSink>);
            }

            SinkConfig::NvrTs { segment_duration_sec: _ , sink_id: _} => {
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
    db_sender: Arc<Sender<DBMessage>>,
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
    db_sender: Arc<Sender<DBMessage>>,
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
