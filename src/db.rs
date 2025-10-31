use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::{constants::*, log};

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
}

pub struct DashcamDb {
    conn: Connection,
}

impl DashcamDb {
    pub fn setup() -> rusqlite::Result<Self> {
        let db_path = PathBuf::from(DB_PATH);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .expect("Failed to create DB directory");
        }

        let db = Self::open(DB_PATH)?;

        let schema_path = PathBuf::from(SCHEMA_PATH);
        let schema_sql = fs::read_to_string(&schema_path)
            .expect("Failed to read schema file");

        db.run_schema(&schema_sql)?;
        Ok(db)
    }

    // open(path/to/dbfile.db)
    pub fn open<P: AsRef<Path>>(path: P) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", &"WAL")?;
        conn.pragma_update(None, "synchronous", &"NORMAL")?;
        conn.pragma_update(None, "foreign_keys", &"ON")?;
        conn.pragma_update(None, "temp_store", &"MEMORY")?;
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

    pub fn current_segment_index(&self) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT value FROM counters WHERE name='segment_index';",
            [],
            |r| r.get(0),
        )
    }

    pub fn increment_segment_index(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE counters SET value=value+1 WHERE name='segment_index';",
            [],
        )?;
        Ok(())
    }

    fn fetch_open_trip(&self) -> rusqlite::Result<Option<Trip>> {
        self.conn
            .query_row(
                "SELECT id, boot_id, start_time_utc, end_time_utc, start_segment, final_segment, start_clock_source, end_clock_source, note
                 FROM trips WHERE final_segment IS NULL ORDER BY id DESC LIMIT 1;",
                [],
                |r| {
                    Ok(Trip {
                        id: r.get(0)?,
                        boot_id: r.get(1)?,
                        start_time_utc: r.get(2)?,
                        end_time_utc: r.get(3)?,
                        start_segment: r.get(4)?,
                        final_segment: r.get::<_, Option<i64>>(5)?,
                        start_clock_source: r.get(6)?,
                        end_clock_source: r.get(7)?,
                        note: r.get(8)?,
                    })
                },
            )
            .optional()
    }

    fn finalize_open_trip(
        &self,
        end_segment_inclusive: i64,
        end_clock_source: &str,
    ) -> rusqlite::Result<()> {
        let now = Self::now();
        self.conn.execute(
            "UPDATE trips
             SET final_segment = ?, end_time_utc = ?, end_clock_source = ?
             WHERE final_segment IS NULL;",
            params![end_segment_inclusive, now, end_clock_source],
        )?;
        Ok(())
    }

    fn insert_trip(
        &self,
        boot_id: &str,
        start_segment: i64,
        start_clock_source: &str,
    ) -> rusqlite::Result<Trip> {
        let now = Self::now();
        self.conn.execute(
            "INSERT INTO trips(boot_id, start_time_utc, start_segment, start_clock_source)
             VALUES(?, ?, ?, ?);",
            params![boot_id, now, start_segment, start_clock_source],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Trip {
            id,
            boot_id: boot_id.to_string(),
            start_time_utc: now,
            end_time_utc: None,
            start_segment,
            final_segment: None,
            start_clock_source: Some(start_clock_source.to_string()),
            end_clock_source: None,
            note: None,
        })
    }

    /// Call on boot/service start.
    pub fn start_or_resume_trip(
        &self,
        boot_id: &str,
        clock_src: &str,
    ) -> rusqlite::Result<Trip> {
        // If you prefer explicit behavior, use:
        // let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let tx = self.conn.unchecked_transaction()?;
        let current: i64 = tx.query_row(
            "SELECT value FROM counters WHERE name='segment_index';",
            [],
            |r| r.get(0),
        )?;

        if let Some((open_id, open_start)) = tx
            .query_row(
                "SELECT id, start_segment FROM trips WHERE final_segment IS NULL ORDER BY id DESC LIMIT 1;",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
        {
            let end_seg = current - 1;
            if end_seg >= open_start {
                tx.execute(
                    "UPDATE trips
                     SET final_segment = ?, end_time_utc = ?, end_clock_source = ?
                     WHERE id = ? AND final_segment IS NULL;",
                    params![end_seg, Self::now(), clock_src, open_id],
                )?;
            }
        }

        tx.execute(
            "INSERT INTO trips(boot_id, start_time_utc, start_segment, start_clock_source)
             VALUES(?, ?, ?, ?);",
            params![boot_id, Self::now(), current, clock_src],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;

        Ok(Trip {
            id,
            boot_id: boot_id.to_string(),
            start_time_utc: Self::now(),
            end_time_utc: None,
            start_segment: current,
            final_segment: None,
            start_clock_source: Some(clock_src.to_string()),
            end_clock_source: None,
            note: None,
        })
    }

    /// MSG: NEW TRIP
    pub fn new_trip(&self, boot_id: &str, clock_src: &str) -> rusqlite::Result<Trip> {
        let tx = self.conn.unchecked_transaction()?;
        let current: i64 = tx.query_row(
            "SELECT value FROM counters WHERE name='segment_index';",
            [],
            |r| r.get(0),
        )?;

        if let Some((open_id, open_start)) = tx
            .query_row(
                "SELECT id, start_segment FROM trips WHERE final_segment IS NULL ORDER BY id DESC LIMIT 1;",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
        {
            let end_seg = current - 1;
            if end_seg >= open_start {
                tx.execute(
                    "UPDATE trips
                     SET final_segment = ?, end_time_utc = ?, end_clock_source = ?
                     WHERE id = ? AND final_segment IS NULL;",
                    params![end_seg, Self::now(), clock_src, open_id],
                )?;
            }
        }

        tx.execute(
            "INSERT INTO trips(boot_id, start_time_utc, start_segment, start_clock_source)
             VALUES(?, ?, ?, ?);",
            params![boot_id, Self::now(), current, clock_src],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;

        Ok(Trip {
            id,
            boot_id: boot_id.to_string(),
            start_time_utc: Self::now(),
            end_time_utc: None,
            start_segment: current,
            final_segment: None,
            start_clock_source: Some(clock_src.to_string()),
            end_clock_source: None,
            note: None,
        })
    }

    /// MSG: SAVE TRIP (close+open trip; do file I/O & ffmpeg outside)
    pub fn save_trip_and_start_new(
        &self,
        boot_id: &str,
        clock_src: &str,
        saved_dir: &str,
    ) -> rusqlite::Result<(Trip, i64, i64, i64)> {
        let tx = self.conn.unchecked_transaction()?;
        let current: i64 = tx.query_row(
            "SELECT value FROM counters WHERE name='segment_index';",
            [],
            |r| r.get(0),
        )?;

        let (closed_id, closed_start) = tx
            .query_row(
                "SELECT id, start_segment FROM trips WHERE final_segment IS NULL ORDER BY id DESC LIMIT 1;",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .optional()?
            .unwrap_or((0, current));

        let closed_end = current - 1;
        if closed_id != 0 && closed_end >= closed_start {
            tx.execute(
                "UPDATE trips
                 SET final_segment = ?, end_time_utc = ?, end_clock_source = ?
                 WHERE id = ? AND final_segment IS NULL;",
                params![closed_end, Self::now(), clock_src, closed_id],
            )?;
            tx.execute(
                "INSERT INTO saved_trips(trip_id, saved_dir, saved_at_utc)
                 VALUES(?, ?, ?);",
                params![closed_id, saved_dir, Self::now()],
            )?;
        }

        tx.execute(
            "INSERT INTO trips(boot_id, start_time_utc, start_segment, start_clock_source)
             VALUES(?, ?, ?, ?);",
            params![boot_id, Self::now(), current, clock_src],
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
            start_clock_source: Some(clock_src.to_string()),
            end_clock_source: None,
            note: None,
        };

        Ok((new_trip, closed_id, closed_start, closed_end))
    }
}
