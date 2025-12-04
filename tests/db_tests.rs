use tempfile::TempDir;

use dashcam_rs::config::{
    CameraConfig, CameraRole, SourceConfig, SourceKind, SinkConfig, GlobalConfig, AppConfig,
};
use dashcam_rs::db::db::DashcamDb;


// Inline the real schema so tests don't depend on disk at runtime.
const SCHEMA_SQL: &str = include_str!("../migrations/0001_init.sql");

/// Helper to build a minimal CameraConfig with one DashcamTs sink.
fn make_test_camera(
    key: &str,
    sink_id: i64,
    segment_duration_sec: u64,
    max_segments: i64,
) -> CameraConfig {
    CameraConfig {
        key: key.to_string(),
        name: format!("Camera {}", key),
        enabled: true,
        role: CameraRole::Dashcam,
        video_width: None,
        video_height: None,
        video_framerate: None,
        source: SourceConfig {
            kind: SourceKind::V4l2,
            rtsp_url: None,
            device: Some("/dev/video0".to_string()),
        },
        sinks: vec![SinkConfig::DashcamTs {
            sink_id,
            segment_duration_sec,
            max_segments,
        }],
    }
}

/// Helper to build a minimal AppConfig for setup_from_config() tests.
fn make_test_app_config(db_path: &str, schema_path: &str) -> AppConfig {
    AppConfig {
        global: GlobalConfig {
            main_dir: ".".to_string(),
            recording_root: "./recordings".to_string(),
            db_path: db_path.to_string(),
            schema_path: schema_path.to_string(),
            log_level: None
        },
        cameras: vec![make_test_camera("cam1", 0, 2, 10)],
    }
}

#[test]
fn it_creates_and_initializes_camera_state() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");

    // Use lower-level setup_with_paths_and_schema so we can pass cameras directly.
    let cameras = vec![make_test_camera("cam1", 0, 2, 10)];
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL, &cameras).unwrap();

    // Resolve camera_id and check initial state
    let camera_id = db.get_camera_id_by_key("cam1").unwrap();
    let sink_id = 0;

    let idx = db.get_segment_index(camera_id, sink_id).unwrap();
    let gent = db.get_segment_generation(camera_id, sink_id).unwrap();
    let abs = db.get_absolute_segments(camera_id, sink_id).unwrap();

    assert_eq!(idx, 0, "fresh DB should start at index 0");
    assert_eq!(gent, 0, "fresh DB should start with generation 0");
    assert_eq!(abs, 0, "fresh DB should start with absolute_segments 0");
}

#[test]
fn increment_wraps_and_bumps_generation_per_sink() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");

    let max_segments = 5;
    let cameras = vec![make_test_camera("cam1", 0, 2, max_segments)];
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL, &cameras).unwrap();

    let camera_id = db.get_camera_id_by_key("cam1").unwrap();
    let sink_id = 0;

    // Advance to max_segments - 1
    for _ in 0..(max_segments - 1) {
        db.increment_segment_index(camera_id, sink_id, max_segments)
            .unwrap();
    }

    let idx_before = db.get_segment_index(camera_id, sink_id).unwrap();
    let gen_before = db.get_segment_generation(camera_id, sink_id).unwrap();
    assert_eq!(idx_before, max_segments - 1);

    // Next increment should wrap to 0 and bump generation
    let idx = db
        .increment_segment_index(camera_id, sink_id, max_segments)
        .unwrap();
    let gen_after = db.get_segment_generation(camera_id, sink_id).unwrap();

    assert_eq!(idx, 0, "index should wrap to 0 at ring boundary");
    assert_eq!(
        gen_after,
        gen_before + 1,
        "generation should increment on wrap"
    );
}

#[test]
fn update_segment_counters_advances_and_wraps_absolute() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");

    let max_segments = 10;
    let cameras = vec![make_test_camera("cam1", 0, 2, max_segments)];
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL, &cameras).unwrap();

    let camera_id = db.get_camera_id_by_key("cam1").unwrap();
    let sink_id = 0;

    // Non-wrapping advance: 0 -> 3
    db.update_segment_counters(camera_id, sink_id, 3, max_segments)
        .unwrap();

    let idx = db.get_segment_index(camera_id, sink_id).unwrap();
    let gent = db.get_segment_generation(camera_id, sink_id).unwrap();
    let abs = db.get_absolute_segments(camera_id, sink_id).unwrap();

    assert_eq!(idx, 3, "segment_index should move to 3");
    assert_eq!(gent, 0, "no wrap yet, generation stays 0");
    assert_eq!(abs, 3, "absolute should be 3 after first advance");

    // Now simulate being near the end of the ring: manually set state to 8, gen=0, abs=8
    db.set_segment_index(camera_id, sink_id, 8).unwrap();
    db.set_segment_generation(camera_id, sink_id, 0).unwrap();
    db.set_absolute_segments(camera_id, sink_id, 8).unwrap();

    // Wrapping advance: 8 -> 2 with max_segments=10
    // diff = (10 - 8) + 2 = 4; abs should go from 8 to 12; gen from 0 to 1
    db.update_segment_counters(camera_id, sink_id, 2, max_segments)
        .unwrap();

    let idx2 = db.get_segment_index(camera_id, sink_id).unwrap();
    let gen2 = db.get_segment_generation(camera_id, sink_id).unwrap();
    let abs2 = db.get_absolute_segments(camera_id, sink_id).unwrap();

    assert_eq!(idx2, 2, "segment_index should be updated to 2 after wrap");
    assert_eq!(gen2, 1, "generation should have incremented on wrap");
    assert_eq!(abs2, 12, "absolute_segments should have advanced by 4");
}

#[test]
fn clamp_segment_index_reduces_to_ring_size() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");

    let max_segments = 10;
    let cameras = vec![make_test_camera("cam1", 0, 2, max_segments)];
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL, &cameras).unwrap();

    let camera_id = db.get_camera_id_by_key("cam1").unwrap();
    let sink_id = 0;

    // Force index to a value larger than max_segments
    db.set_segment_index(camera_id, sink_id, 25).unwrap();

    db.clamp_segment_index(camera_id, sink_id, max_segments)
        .unwrap();

    let idx = db.get_segment_index(camera_id, sink_id).unwrap();
    assert_eq!(idx, 25 % max_segments, "segment_index should be clamped mod max_segments");
}

#[test]
fn setup_from_config_works_with_appconfig() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let schema_path = tmp.path().join("schema.sql");

    // Write schema to a temp file so AppConfig can refer to it by path
    std::fs::write(&schema_path, SCHEMA_SQL).unwrap();

    let cfg = make_test_app_config(
        db_path.to_string_lossy().as_ref(),
        schema_path.to_string_lossy().as_ref(),
    );

    let db = DashcamDb::setup_from_config(&cfg).unwrap();

    // Check that the camera_from_config exists and has state
    let camera_id = db.get_camera_id_by_key("cam1").unwrap();
    let idx = db.get_segment_index(camera_id, 0).unwrap();
    assert_eq!(idx, 0);
}
