use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub global: GlobalConfig,
    pub cameras: Vec<CameraConfig>,
}

#[derive(Debug, Deserialize)]
pub struct GlobalConfig {
    pub main_dir: String,
    pub recording_root: String,
    pub db_path: String,
    pub schema_path: String,
    pub log_level: Option<String>,

    pub video_width: Option<i64>,
    pub video_height: Option<i64>,
    pub video_framerate: Option<i64>
}

#[derive(Debug, Deserialize)]
pub struct CameraConfig {
    pub key: String,         // "dashcam", "cam_front", etc.
    pub name: String,
    pub enabled: bool,
    pub role: CameraRole,

    pub source: SourceConfig,
    pub sinks: Vec<SinkConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CameraRole {
    Dashcam,
    Nvr,
    Preview,
}

#[derive(Debug, Deserialize)]
pub struct SourceConfig {
    pub kind: SourceKind,
    pub rtsp_url: Option<String>,
    pub device: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Libcamera,
    Rtsp,
    V4l2,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SinkConfig {
    DashcamTs { max_segments: i64, segment_duration_sec: u64 , sink_id: i64},
    NvrTs { segment_duration_sec: u64 , sink_id: i64},
    Hls { segment_duration_sec: u64 , sink_id: i64},
}
