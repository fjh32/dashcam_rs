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
    pub log_level: Option<String>
}

#[derive(Debug, Deserialize)]
pub struct CameraConfig {
    pub key: String,
    pub name: String,
    pub enabled: bool,
    pub role: CameraRole,

    pub video_width: Option<i64>,
    pub video_height: Option<i64>,
    pub video_framerate: Option<i64>,

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

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
pub struct SourceConfig {
    pub kind: SourceKind,
    pub rtsp_url: Option<String>,
    pub device: Option<String>,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
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


pub fn verify_app_config(app_config: &AppConfig) -> bool {
    let mut checklist : Vec<SourceConfig> = vec![];
    for camera_config in app_config.cameras.iter() {
        let camera_source = &camera_config.source;
        // Can't have 2 cameras with the same source
        for item in checklist.iter() {
            if *item == *camera_source {
                return false;
            }
        }
        // Rtsp type needs rtsp url
        if camera_source.kind == SourceKind::Rtsp && camera_source.rtsp_url == None {
            return false;
        }
        // V4L2 needs a device
        if camera_source.kind == SourceKind::V4l2 && camera_source.device == None {
            return false;
        }
        checklist.push(camera_source.clone());
    }

    true
}