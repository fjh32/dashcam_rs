use tempfile::TempDir;

use dashcam_rs::db::DashcamDb;
// If your module path differs, adjust the import accordingly.

// Inline the real schema so tests don’t touch disk:
const SCHEMA_SQL: &str = include_str!("../migrations/0001_init.sql");

// If SEGMENTS_TO_KEEP is in crate::constants, you can pull it, or hardcode a small test value
// but better to use the real one:
use dashcam_rs::constants::SEGMENTS_TO_KEEP;

#[test]
fn it_creates_and_migrates() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL).unwrap();

    let idx = db.get_segment_index(1).unwrap();
    assert_eq!(idx, 0, "fresh DB should start at index 0");
}

#[test]
fn new_trip_starts_at_current_index() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL).unwrap();

    // index is 0 in a fresh DB
    let t = db.new_trip(1).unwrap();
    assert_eq!(t.start_segment, 0);

    // If we increment a few times, the next new_trip should start at the new index
    for _ in 0..5 {
        db.increment_segment_index(1).unwrap();
    }
    let t2 = db.new_trip(1).unwrap();
    assert_eq!(t2.start_segment, 5);
}

#[test]
fn increment_wraps_and_bumps_generation() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL).unwrap();

    // Advance to SEGMENTS_TO_KEEP - 1
    for _ in 0..(SEGMENTS_TO_KEEP - 1) {
        db.increment_segment_index(1).unwrap();
    }
    let before_gen = db.get_segment_generation(1).unwrap();

    // Next increment should wrap to 0 and bump generation
    let idx = db.increment_segment_index(1).unwrap();
    let after_gen = db.get_segment_generation(1).unwrap();

    assert_eq!(idx, 0, "index should wrap to 0");
    assert_eq!(
        after_gen,
        before_gen + 1,
        "generation should increment on wrap"
    );
}

#[test]
fn eviction_flag_logic_marks_old_trips() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL).unwrap();

    // Start a trip, record a few segments
    let t = db.new_trip(1).unwrap();
    for _ in 0..5 {
        db.increment_segment_index(1).unwrap();
    }

    // Close it at current-1
    let current = db.get_segment_index(1).unwrap();
    db.finalize_open_trip(1,current - 1).unwrap();

    // Write more than a full ring to guarantee eviction
    for _ in 0..(SEGMENTS_TO_KEEP + 10) {
        db.increment_segment_index(1).unwrap();
    }

    // Mark evicted trips
    let updated = db.mark_fully_evicted_trips(1).unwrap();
    assert!(updated >= 1, "at least one trip should be marked evicted");

    // Direct check
    assert!(db.is_trip_fully_evicted(t.id).unwrap());
}

#[test]
fn save_trip_and_start_new_records_saved_trips() {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("db.sqlite");
    let db = DashcamDb::setup_with_paths_and_schema(&db_path, SCHEMA_SQL).unwrap();

    // Make a few segments so closing is valid
    for _ in 0..3 {
        db.increment_segment_index(1).unwrap();
    }

    let (_new_trip, closed_id, closed_start, closed_end) = db
        .save_trip_and_start_new( 1,"/tmp/saved-tests")
        .unwrap();

    if closed_id != 0 {
        assert!(closed_end >= closed_start);
        let cnt: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM saved_trips WHERE trip_id=?1;",
                rusqlite::params![closed_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            cnt, 1,
            "saved_trips should contain a row for the closed trip"
        );
    }
}
