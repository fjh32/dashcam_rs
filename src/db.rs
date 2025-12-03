use crate::constants::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Trip {
    pub id: i64,
    pub boot_id: String,
    pub start_time_utc: i64,
    pub end_time_utc: Option<i64>,
    pub start_segment: i64,
    pub final_segment: Option<i64>,
    pub start_clock_source: Option<String>,
    pub end_clock_source: Option<String>,
    pub note: Option<String>,
    pub start_gen: i64,
    pub end_gen: Option<i64>,
    pub fully_evicted: bool,
    pub evicted_at_utc: Option<i64>,
}

pub struct DashcamDb {
    pub conn: Connection,
}

fn get_boot_id() -> String {
    if let Ok(boot_id) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().to_string())
    {
        boot_id
    } else {
        "unknown_boot_id".to_string()
    }
}

impl DashcamDb {
    //////////// helper DB connection setups
    pub fn setup_with_paths_and_schema<P: AsRef<std::path::Path>>(
        db_path: P,
        schema_sql: &str,
    ) -> rusqlite::Result<Self> {
        let db_path = db_path.as_ref();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create DB directory");
        }
        let db = Self::open(db_path)?;
        db.run_schema(schema_sql)?;
        db.ensure_counters_initialized()?; // now seeds cameras + camera_state
        db.clamp_segment_index(1)?; // keep ring index in range if constant changed
        db.mark_fully_evicted_trips(1)?;
        Ok(db)
    }

    pub fn setup_with_paths_and_schema_file<
        P: AsRef<std::path::Path>,
        Q: AsRef<std::path::Path>,
    >(
        db_path: P,
        schema_path: Q,
    ) -> rusqlite::Result<Self> {
        let schema_sql = std::fs::read_to_string(&schema_path).expect("Failed to read schema file");
        Self::setup_with_paths_and_schema(db_path, &schema_sql)
    }

    /// setup with constants.rs
    pub fn setup() -> rusqlite::Result<Self> {
        let db_path = PathBuf::from(DB_PATH);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create DB directory");
        }

        let db = Self::open(DB_PATH)?;

        let schema_sql = fs::read_to_string(SCHEMA_PATH).expect("Failed to read schema file");

        db.run_schema(&schema_sql)?;
        db.ensure_counters_initialized()?; // seeds 'dashcam' camera + camera_state
        db.clamp_segment_index(1)?; // keep ring index in range if constant changed
        db.mark_fully_evicted_trips(1)?;
        Ok(db)
    }

    pub fn open<P: AsRef<Path>>(path: P) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", &"WAL")?;
        conn.pragma_update(None, "synchronous", &"NORMAL")?;
        conn.pragma_update(None, "foreign_keys", &"ON")?;
        conn.pragma_update(None, "temp_store", &"MEMORY")?;
        conn.busy_timeout(Duration::from_millis(100))?;
        Ok(Self { conn })
    }

    pub fn run_schema(&self, schema_sql: &str) -> rusqlite::Result<()> {
        self.conn.execute_batch(schema_sql)?;
        Ok(())
    }

    #[inline]
    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// Helper: get the camera_id for the single dashcam camera.
    /// Assumes schema has inserted a row with key='dashcam'.
    fn get_dashcam_camera_id(&self) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT id FROM cameras WHERE key='dashcam';",
            [],
            |r| r.get(0),
        )
    }

    /// Seed base rows for the new schema:
    /// - ensure there's a 'dashcam' camera row
    /// - ensure there's a matching camera_state row (segment counters)
    ///
    /// The name is kept for compatibility with existing call sites.
    pub fn ensure_counters_initialized(&self) -> rusqlite::Result<()> {
        // Insert the dashcam camera if it doesn't exist
        self.conn.execute(
            "INSERT INTO cameras (key, name)
             VALUES ('dashcam', 'Dashcam Front')
             ON CONFLICT(key) DO NOTHING;",
            [],
        )?;

        // Insert camera_state row if it doesn't exist
        self.conn.execute(
            "INSERT INTO camera_state (camera_id, segment_index, segment_generation, absolute_segments)
             VALUES (
               (SELECT id FROM cameras WHERE key='dashcam'),
               0, 0, 0
             )
             ON CONFLICT(camera_id) DO NOTHING;",
            [],
        )?;

        Ok(())
    }

    // ====== GETTERS for segment counters (from camera_state) ======

    pub fn get_segment_index(&self, camera_id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT segment_index
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )
    }

    pub fn get_segment_generation(&self, camera_id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT segment_generation
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )
    }

    pub fn get_absolute_segments(&self, camera_id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT absolute_segments
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )
    }

    // ====== SETTERS for segment counters (camera_state) ======

    pub fn set_segment_index(&self, camera_id: i64, value: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
            SET segment_index = ?
            WHERE camera_id = ?;",
            params![value, camera_id],
        )?;
        Ok(())
    }

    pub fn set_segment_generation(&self, camera_id: i64, value: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
            SET segment_generation = ?
            WHERE camera_id = ?;",
            params![value, camera_id],
        )?;
        Ok(())
    }

    pub fn set_absolute_segments(&self, camera_id: i64, value: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
            SET absolute_segments = ?
            WHERE camera_id = ?;",
            params![value, camera_id],
        )?;
        Ok(())
    }


    /// Update segment_index, segment_generation, and absolute_segments
    /// for the given camera_id, based on a new segment index coming
    /// from that camera's recording pipeline.
    ///
    /// Assumptions:
    /// - `new_segment_index` is the current in-memory index managed by the sink.
    /// - It advances by 1 each new segment, wrapping at SEGMENTS_TO_KEEP.
    pub fn update_segment_counters_from_index(
        &self,
        camera_id: i64,
        new_segment_index: i64,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        // Load current counters for this camera
        let (cur_idx, cur_gen, cur_abs): (i64, i64, i64) = tx.query_row(
            "SELECT segment_index, segment_generation, absolute_segments
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        // If DB already matches, nothing to do
        if new_segment_index == cur_idx {
            tx.commit()?;
            return Ok(());
        }

        let max = SEGMENTS_TO_KEEP as i64;

        // Determine wrap:
        // Example: 86399 -> 0 when max == 86400
        let wrapped = new_segment_index < cur_idx;

        // Determine number of segments advanced
        let diff = if wrapped {
            (max - cur_idx) + new_segment_index
        } else {
            new_segment_index - cur_idx
        };

        // Update segment_index
        tx.execute(
            "UPDATE camera_state
            SET segment_index = ?
            WHERE camera_id = ?;",
            params![new_segment_index, camera_id],
        )?;

        // Bump generation if we wrapped
        if wrapped {
            tx.execute(
                "UPDATE camera_state
                SET segment_generation = ?
                WHERE camera_id = ?;",
                params![cur_gen + 1, camera_id],
            )?;
        }

        // absolute_segments always increases by `diff`
        tx.execute(
            "UPDATE camera_state
            SET absolute_segments = ?
            WHERE camera_id = ?;",
            params![cur_abs + diff, camera_id],
        )?;

        tx.commit()?;
        Ok(())
    }

    /// Increment ring index and bump generation/absolute atomically
    /// for the given camera. Returns new ring index (post-increment).
    pub fn increment_segment_index(&self, camera_id: i64) -> rusqlite::Result<i64> {
        let tx = self.conn.unchecked_transaction()?;

        let (cur_idx, cur_gen, cur_abs): (i64, i64, i64) = tx.query_row(
            "SELECT segment_index, segment_generation, absolute_segments
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        let (next_idx, next_gen, wrapped) = if cur_idx + 1 >= SEGMENTS_TO_KEEP {
            (0, cur_gen + 1, true)
        } else {
            (cur_idx + 1, cur_gen, false)
        };

        tx.execute(
            "UPDATE camera_state
            SET segment_index = ?
            WHERE camera_id = ?;",
            params![next_idx, camera_id],
        )?;
        if wrapped {
            tx.execute(
                "UPDATE camera_state
                SET segment_generation = ?
                WHERE camera_id = ?;",
                params![next_gen, camera_id],
            )?;
        }
        tx.execute(
            "UPDATE camera_state
            SET absolute_segments = ?
            WHERE camera_id = ?;",
            params![cur_abs + 1, camera_id],
        )?;

        tx.commit()?;
        Ok(next_idx)
    }


    pub fn clamp_segment_index(&self, camera_id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
            SET segment_index = segment_index % ?1
            WHERE camera_id = ?2;",
            params![SEGMENTS_TO_KEEP, camera_id],
        )?;
        Ok(())
    }


    /// Mark trips whose files are certainly overwritten by the ring.
    /// Uses camera_state.absolute_segments to be robust to SEGMENTS_TO_KEEP changes.
    ///
    /// Call this function on a schedule or every so often somewhere else....
    /// Mark trips whose files are certainly overwritten by the ring
    /// for the given camera.
    pub fn mark_fully_evicted_trips(&self, camera_id: i64) -> rusqlite::Result<usize> {
        let tx = self.conn.unchecked_transaction()?;

        let abs_latest: i64 = tx.query_row(
            "SELECT absolute_segments
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )?;

        // earliest absolute index that still exists on disk
        let abs_earliest = (abs_latest - (SEGMENTS_TO_KEEP - 1)).max(0);

        let updated = tx.execute(
            "UPDATE trips
            SET fully_evicted = 1,
                evicted_at_utc = CAST(strftime('%s','now') AS INTEGER)
            WHERE fully_evicted = 0
            AND camera_id = ?3
            AND final_segment IS NOT NULL
            AND ((COALESCE(end_gen, start_gen) * ?1) + final_segment) < ?2;",
            params![SEGMENTS_TO_KEEP, abs_earliest, camera_id],
        )?;

        tx.commit()?;
        Ok(updated)
    }

    // ---- trips ----

    pub fn fetch_open_trip(&self, camera_id: i64) -> rusqlite::Result<Option<Trip>> {
        self.conn
            .query_row(
                "SELECT id, boot_id, start_time_utc, end_time_utc,
                        start_segment, final_segment,
                        start_clock_source, end_clock_source, note,
                        start_gen, end_gen, fully_evicted, evicted_at_utc
                FROM trips
                WHERE final_segment IS NULL
                AND camera_id = ?1
                ORDER BY id DESC
                LIMIT 1;",
                params![camera_id],
                |r| {
                    Ok(Trip {
                        id: r.get(0)?,
                        boot_id: r.get(1)?,
                        start_time_utc: r.get(2)?,
                        end_time_utc: r.get(3)?,
                        start_segment: r.get(4)?,
                        final_segment: r.get(5)?,
                        start_clock_source: r.get(6)?,
                        end_clock_source: r.get(7)?,
                        note: r.get(8)?,
                        start_gen: r.get(9)?,
                        end_gen: r.get(10)?,
                        fully_evicted: r.get(11)?,
                        evicted_at_utc: r.get(12)?,
                    })
                },
            )
            .optional()
    }


    pub fn finalize_open_trip(
        &self,
        camera_id: i64,
        end_segment_inclusive: i64,
    ) -> rusqlite::Result<()> {
        let now = Self::now();
        let cur_gen: i64 = self.get_segment_generation(camera_id)?;
        self.conn.execute(
            "UPDATE trips
            SET final_segment = ?, end_time_utc = ?, end_clock_source = ?, end_gen = ?
            WHERE final_segment IS NULL
            AND camera_id = ?;",
            params![end_segment_inclusive, now, "boot", cur_gen, camera_id],
        )?;
        Ok(())
    }

    pub fn insert_trip(
        &self,
        camera_id: i64,
        start_segment: i64,
    ) -> rusqlite::Result<Trip> {
        let now = Self::now();
        let boot_id = get_boot_id();
        let start_gen: i64 = self.get_segment_generation(camera_id)?;

        self.conn.execute(
            "INSERT INTO trips(camera_id, boot_id, start_time_utc, start_segment, start_clock_source, start_gen)
            VALUES(?, ?, ?, ?, ?, ?);",
            params![camera_id, boot_id, now, start_segment, "boot", start_gen],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Trip {
            id,
            boot_id: boot_id.to_string(),
            start_time_utc: now,
            end_time_utc: None,
            start_segment,
            final_segment: None,
            start_clock_source: Some("boot".to_string()),
            end_clock_source: None,
            note: None,
            start_gen,
            end_gen: None,
            fully_evicted: false,
            evicted_at_utc: None,
        })
    }

    /// MSG: NEW TRIP
    /// - edit current trip's row with FINAL_SEGMENT.
    /// - create new Trip row in table
    /// Close the current open trip (if any) for this camera, and open a new one.
    pub fn new_trip(&self, camera_id: i64) -> rusqlite::Result<Trip> {
        let tx = self.conn.unchecked_transaction()?;
        let boot_id = get_boot_id();

        let current: i64 = tx.query_row(
            "SELECT segment_index
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )?;
        let cur_gen: i64 = tx.query_row(
            "SELECT segment_generation
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )?;

        if let Some((open_id, open_start)) = tx
            .query_row(
                "SELECT id, start_segment
                FROM trips
                WHERE final_segment IS NULL
                AND camera_id = ?1
                ORDER BY id DESC
                LIMIT 1;",
                params![camera_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
        {
            let end_seg = current - 1;
            if end_seg >= open_start {
                tx.execute(
                    "UPDATE trips
                    SET final_segment = ?, end_time_utc = ?, end_clock_source = ?, end_gen = ?
                    WHERE id = ? AND final_segment IS NULL;",
                    params![end_seg, Self::now(), "boot", cur_gen, open_id],
                )?;
            }
        }

        tx.execute(
            "INSERT INTO trips(camera_id, boot_id, start_time_utc, start_segment, start_clock_source, start_gen)
            VALUES(?, ?, ?, ?, ?, ?);",
            params![camera_id, boot_id, Self::now(), current, "boot", cur_gen],
        )?;
        let new_id = tx.last_insert_rowid();
        tx.commit()?;

        Ok(Trip {
            id: new_id,
            boot_id: boot_id.to_string(),
            start_time_utc: Self::now(),
            end_time_utc: None,
            start_segment: current,
            final_segment: None,
            start_clock_source: Some("boot".to_string()),
            end_clock_source: None,
            note: None,
            start_gen: cur_gen,
            end_gen: None,
            fully_evicted: false,
            evicted_at_utc: None,
        })
    }


    /// MSG: SAVE TRIP (close+open trip; do file I/O & ffmpeg outside)
    ///     - Same process as NEW TRIP
    ///     - I/O: previous trip, save all files associated with the trip in save/ dir
    ///     - Copy TRIP row from TRIPS table to SAVED_TRIPS table with dir to all saved videos.
    ///     - I/O: stitch together all .ts files from trip into a single mp4 with ffmpeg
    pub fn save_trip_and_start_new(
        &self,
        camera_id: i64,
        saved_dir: &str,
    ) -> rusqlite::Result<(Trip, i64, i64, i64)> {
        let tx = self.conn.unchecked_transaction()?;
        let boot_id = get_boot_id();

        let current: i64 = tx.query_row(
            "SELECT segment_index
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )?;
        let cur_gen: i64 = tx.query_row(
            "SELECT segment_generation
            FROM camera_state
            WHERE camera_id = ?1;",
            params![camera_id],
            |r| r.get(0),
        )?;

        let (closed_id, closed_start) = tx
            .query_row(
                "SELECT id, start_segment
                FROM trips
                WHERE final_segment IS NULL
                AND camera_id = ?1
                ORDER BY id DESC
                LIMIT 1;",
                params![camera_id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
            .unwrap_or((0, current));

        let closed_end = current - 1;
        if closed_id != 0 && closed_end >= closed_start {
            tx.execute(
                "UPDATE trips
                SET final_segment = ?, end_time_utc = ?, end_clock_source = ?, end_gen = ?
                WHERE id = ? AND final_segment IS NULL;",
                params![closed_end, Self::now(), "boot", cur_gen, closed_id],
            )?;
            tx.execute(
                "INSERT INTO saved_trips(trip_id, saved_dir, saved_at_utc)
                VALUES(?, ?, ?);",
                params![closed_id, saved_dir, Self::now()],
            )?;
        }

        tx.execute(
            "INSERT INTO trips(camera_id, boot_id, start_time_utc, start_segment, start_clock_source, start_gen)
            VALUES(?, ?, ?, ?, ?, ?);",
            params![camera_id, boot_id, Self::now(), current, "boot", cur_gen],
        )?;
        let new_id = tx.last_insert_rowid();
        tx.commit()?;

        let new_trip = Trip {
            id: new_id,
            boot_id: boot_id.to_string(),
            start_time_utc: Self::now(),
            end_time_utc: None,
            start_segment: current,
            final_segment: None,
            start_clock_source: Some("boot".to_string()),
            end_clock_source: None,
            note: None,
            start_gen: cur_gen,
            end_gen: None,
            fully_evicted: false,
            evicted_at_utc: None,
        };

        Ok((new_trip, closed_id, closed_start, closed_end))
    }

    // ---- queries for UI / maintenance ----

    /// Fast check for a single trip’s eviction status using absolute_segments.
    pub fn is_trip_fully_evicted(&self, trip_id: i64) -> rusqlite::Result<bool> {
        let (abs_latest, fully_evicted, final_seg_opt, end_gen_or_start): (
            i64,
            bool,
            Option<i64>,
            i64,
        ) = self.conn.query_row(
            "SELECT
                 cs.absolute_segments AS abs_latest,
                 t.fully_evicted,
                 t.final_segment,
                 COALESCE(t.end_gen, t.start_gen)
             FROM trips t
             JOIN camera_state cs ON cs.camera_id = t.camera_id
             WHERE t.id = ?1;",
            params![trip_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;

        if fully_evicted {
            return Ok(true);
        }
        let final_seg = match final_seg_opt {
            Some(v) => v,
            None => return Ok(false), // still open -> not fully evicted
        };

        let abs_earliest = (abs_latest - (SEGMENTS_TO_KEEP - 1)).max(0);
        let abs_end = end_gen_or_start.saturating_mul(SEGMENTS_TO_KEEP) + final_seg;

        Ok(abs_end < abs_earliest)
    }

    /// Return all trips that are not flagged as fully evicted (good for timeline UI).
    pub fn list_active_trips(&self, camera_id: i64) -> rusqlite::Result<Vec<Trip>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, boot_id, start_time_utc, end_time_utc,
                    start_segment, final_segment,
                    start_clock_source, end_clock_source, note,
                    start_gen, end_gen, fully_evicted, evicted_at_utc
            FROM trips
            WHERE fully_evicted = 0
            AND camera_id = ?
            ORDER BY id DESC;",
        )?;

        let rows = stmt
            .query_map(params![camera_id], |r| {
                Ok(Trip {
                    id: r.get(0)?,
                    boot_id: r.get(1)?,
                    start_time_utc: r.get(2)?,
                    end_time_utc: r.get(3)?,
                    start_segment: r.get(4)?,
                    final_segment: r.get(5)?,
                    start_clock_source: r.get(6)?,
                    end_clock_source: r.get(7)?,
                    note: r.get(8)?,
                    start_gen: r.get(9)?,
                    end_gen: r.get(10)?,
                    fully_evicted: r.get(11)?,
                    evicted_at_utc: r.get(12)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(rows)
    }

}
