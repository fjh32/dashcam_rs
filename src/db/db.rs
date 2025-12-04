use crate::config::{AppConfig, CameraConfig, SinkConfig};

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct DashcamDb {
    pub conn: Connection,
}

impl DashcamDb {
    ////////////////////////////////////////////////////////////////////////////////
    // Setup / initialization
    ////////////////////////////////////////////////////////////////////////////////

    /// Main setup entry point using AppConfig:
    ///
    /// - ensure DB directory exists
    /// - open DB
    /// - run schema from `global.schema_path`
    /// - insert/update cameras from config (key, name, rtsp_url)
    /// - ensure `camera_state` rows exist for each camera
    pub fn setup_from_config(cfg: &AppConfig) -> Result<Self> {
        let db_path = PathBuf::from(&cfg.global.db_path);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create DB directory {:?}", parent))?;
        }

        let db = Self::open(&db_path)
            .with_context(|| format!("Failed to open DB at {:?}", db_path))?;

        let schema_sql = fs::read_to_string(&cfg.global.schema_path)
            .with_context(|| format!("Failed to read schema file {}", cfg.global.schema_path))?;

        db.run_schema(&schema_sql)
            .context("Failed to run schema.sql")?;

        db.ensure_cameras_initialized(&cfg.cameras)
            .context("Failed to initialize cameras from config")?;

        Ok(db)
    }

    /// Lower-level setup that takes explicit paths & schema SQL.
    pub fn setup_with_paths_and_schema<P: AsRef<Path>>(
        db_path: P,
        schema_sql: &str,
        cameras: &[CameraConfig],
    ) -> Result<Self> {
        let db_path = db_path.as_ref();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create DB directory {:?}", parent))?;
        }

        let db = Self::open(db_path)
            .with_context(|| format!("Failed to open DB at {:?}", db_path))?;

        db.run_schema(schema_sql).context("Failed to run schema")?;
        db.ensure_cameras_initialized(cameras)
            .context("Failed to initialize cameras")?;

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

    ////////////////////////////////////////////////////////////////////////////////
    // Camera + camera_state initialization
    ////////////////////////////////////////////////////////////////////////////////

    /// Ensure every camera from the config exists in `cameras` and that
    /// each DashcamTs sink has a corresponding row in `camera_state`.
    ///
    /// - Inserts/updates cameras (key, name, rtsp_url).
    /// - For each DashcamTs sink, inserts a (camera_id, sink_id) row into camera_state.
    pub fn ensure_cameras_initialized(
        &self,
        cameras: &[CameraConfig],
    ) -> rusqlite::Result<()> {
        for cam in cameras {
            let rtsp_url = cam.source.rtsp_url.as_deref();

            // Upsert camera row
            self.conn.execute(
                "INSERT INTO cameras (key, name, rtsp_url)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    name     = excluded.name,
                    rtsp_url = COALESCE(excluded.rtsp_url, cameras.rtsp_url);",
                rusqlite::params![cam.key, cam.name, rtsp_url],
            )?;

            // For each DashcamTs sink, ensure a camera_state row exists
            for sink in &cam.sinks {
                if let SinkConfig::DashcamTs {
                    sink_id,
                    ..
                } = sink
                {
                    self.conn.execute(
                        "INSERT INTO camera_state (
                             camera_id,
                             sink_id,
                             segment_index,
                             segment_generation,
                             absolute_segments
                         )
                         VALUES (
                             (SELECT id FROM cameras WHERE key = ?1),
                             ?2,
                             0, 0, 0
                         )
                         ON CONFLICT(camera_id, sink_id) DO NOTHING;",
                        rusqlite::params![cam.key, sink_id],
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Helper: resolve camera_id from camera key.
    ///
    /// Call this once at startup / pipeline construction and store the ID in your sink.
    pub fn get_camera_id_by_key(&self, camera_key: &str) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT id FROM cameras WHERE key = ?1;",
            params![camera_key],
            |r| r.get(0),
        )
    }

    ////////////////////////////////////////////////////////////////////////////////
    // Segment counters API (ID-based, hot path)
    ////////////////////////////////////////////////////////////////////////////////

    pub fn get_segment_index(
        &self,
        camera_id: i64,
        sink_id: i64,
    ) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT segment_index
             FROM camera_state
             WHERE camera_id = ?1 AND sink_id = ?2;",
            params![camera_id, sink_id],
            |r| r.get(0),
        )
    }

    pub fn get_segment_generation(
        &self,
        camera_id: i64,
        sink_id: i64,
    ) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT segment_generation
             FROM camera_state
             WHERE camera_id = ?1 AND sink_id = ?2;",
            params![camera_id, sink_id],
            |r| r.get(0),
        )
    }

    pub fn get_absolute_segments(
        &self,
        camera_id: i64,
        sink_id: i64,
    ) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT absolute_segments
             FROM camera_state
             WHERE camera_id = ?1 AND sink_id = ?2;",
            params![camera_id, sink_id],
            |r| r.get(0),
        )
    }

    // ====== SETTERS for segment counters (camera_state) ======

    pub fn set_segment_index(
        &self,
        camera_id: i64,
        sink_id: i64,
        value: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
             SET segment_index = ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            params![value, camera_id, sink_id],
        )?;
        Ok(())
    }

    pub fn set_segment_generation(
        &self,
        camera_id: i64,
        sink_id: i64,
        value: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
             SET segment_generation = ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            params![value, camera_id, sink_id],
        )?;
        Ok(())
    }

    pub fn set_absolute_segments(
        &self,
        camera_id: i64,
        sink_id: i64,
        value: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
             SET absolute_segments = ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            params![value, camera_id, sink_id],
        )?;
        Ok(())
    }

    /// Update segment_index, segment_generation, and absolute_segments
    /// for the given (camera_id, sink_id) based on a new ring index.
    pub fn update_segment_counters(
        &self,
        camera_id: i64,
        sink_id: i64,
        new_segment_index: i64,
        max_segments: i64,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;

        let (cur_idx, cur_gen, cur_abs): (i64, i64, i64) = tx.query_row(
            "SELECT segment_index, segment_generation, absolute_segments
             FROM camera_state
             WHERE camera_id = ?1 AND sink_id = ?2;",
            rusqlite::params![camera_id, sink_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        // If DB already matches, nothing to do.
        if new_segment_index == cur_idx {
            tx.commit()?;
            return Ok(());
        }

        let max = max_segments;
        let wrapped = new_segment_index < cur_idx;

        // How far did we advance around the ring?
        let diff = if wrapped {
            (max - cur_idx) + new_segment_index
        } else {
            new_segment_index - cur_idx
        };

        // Update segment_index
        tx.execute(
            "UPDATE camera_state
             SET segment_index = ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            rusqlite::params![new_segment_index, camera_id, sink_id],
        )?;

        // Bump generation on wrap
        if wrapped {
            tx.execute(
                "UPDATE camera_state
                 SET segment_generation = ?1
                 WHERE camera_id = ?2 AND sink_id = ?3;",
                rusqlite::params![cur_gen + 1, camera_id, sink_id],
            )?;
        }

        // absolute_segments always increases by `diff`
        tx.execute(
            "UPDATE camera_state
             SET absolute_segments = ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            rusqlite::params![cur_abs + diff, camera_id, sink_id],
        )?;

        tx.commit()?;
        Ok(())
    }


    /// Increment ring index and bump generation/absolute atomically
    /// for the given (camera_id, sink_id). Returns new ring index.
    pub fn increment_segment_index(
        &self,
        camera_id: i64,
        sink_id: i64,
        max_segments: i64,
    ) -> rusqlite::Result<i64> {
        let tx = self.conn.unchecked_transaction()?;

        let (cur_idx, cur_gen, cur_abs): (i64, i64, i64) = tx.query_row(
            "SELECT segment_index, segment_generation, absolute_segments
             FROM camera_state
             WHERE camera_id = ?1 AND sink_id = ?2;",
            params![camera_id, sink_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;

        let (next_idx, next_gen, wrapped) = if cur_idx + 1 >= max_segments {
            (0, cur_gen + 1, true)
        } else {
            (cur_idx + 1, cur_gen, false)
        };

        tx.execute(
            "UPDATE camera_state
             SET segment_index = ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            params![next_idx, camera_id, sink_id],
        )?;

        if wrapped {
            tx.execute(
                "UPDATE camera_state
                 SET segment_generation = ?1
                 WHERE camera_id = ?2 AND sink_id = ?3;",
                params![next_gen, camera_id, sink_id],
            )?;
        }

        tx.execute(
            "UPDATE camera_state
             SET absolute_segments = ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            params![cur_abs + 1, camera_id, sink_id],
        )?;

        tx.commit()?;
        Ok(next_idx)
    }

    ////////////////////////////////////////////////////////////////////////////////
    // Clamping helpers
    ////////////////////////////////////////////////////////////////////////////////

    /// Clamp ring index for a single (camera_id, sink_id) to `max_segments`.
    pub fn clamp_segment_index(
        &self,
        camera_id: i64,
        sink_id: i64,
        max_segments: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
             SET segment_index = segment_index % ?1
             WHERE camera_id = ?2 AND sink_id = ?3;",
            params![max_segments, camera_id, sink_id],
        )?;
        Ok(())
    }

    /// Clamp ring index for *all* camera_state rows (useful if sharing a global ring size).
    /// If different sinks use different ring sizes, call `clamp_segment_index` per sink instead.
    pub fn clamp_all_segment_indices(&self, max_segments: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE camera_state
             SET segment_index = segment_index % ?1;",
            params![max_segments],
        )?;
        Ok(())
    }
}
