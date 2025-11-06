pub const SOCKET_PATH: &str = "/tmp/dashcam.sock";

pub const MAIN_DIR: &str = "/var/lib/dashcam/";
pub const DB_PATH: &str = "/var/lib/dashcam/dashcam.db";
pub const SCHEMA_PATH: &str = "/var/lib/dashcam/0001_init.sql";

// DEBUG
#[cfg(debug_assertions)]
pub const VIDEO_DURATION: u64 = 2;
#[cfg(debug_assertions)]
pub const VIDEO_WIDTH: i32 = 640;
#[cfg(debug_assertions)]
pub const VIDEO_HEIGHT: i32 = 480;
#[cfg(debug_assertions)]
pub const VIDEO_FRAMERATE: i32 = 10;
#[cfg(debug_assertions)]
pub const RECORDING_DIR: &str = "./recordings/";
#[cfg(debug_assertions)]
pub const RECORDING_SAVE_DIR: &str = "./recordings/save/";
#[cfg(debug_assertions)]
pub const SEGMENTS_TO_KEEP: i64 = 86400 / 2 * 2; // 2 days worth

// RELEASE
#[cfg(not(debug_assertions))]
pub const VIDEO_DURATION: u64 = 2;
#[cfg(not(debug_assertions))]
pub const VIDEO_WIDTH: i32 = 1920;
#[cfg(not(debug_assertions))]
pub const VIDEO_HEIGHT: i32 = 1080;
#[cfg(not(debug_assertions))]
pub const VIDEO_FRAMERATE: i32 = 30;
#[cfg(not(debug_assertions))]
pub const RECORDING_DIR: &str = "/var/lib/dashcam/recordings/";
#[cfg(not(debug_assertions))]
pub const RECORDING_SAVE_DIR: &str = "/var/lib/dashcam/recordings/save/";
#[cfg(not(debug_assertions))]
pub const SEGMENTS_TO_KEEP: i64 = 86400 / 2 * 2; // 2 days worth
