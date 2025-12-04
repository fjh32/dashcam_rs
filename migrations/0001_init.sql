PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA temp_store=MEMORY;

----------------------------------------------------------------------
-- Logical cameras
-- Each dashcam / IP cam / preview stream is a "camera" row
----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS cameras (
  id    INTEGER PRIMARY KEY,
  key   TEXT NOT NULL UNIQUE,   -- e.g. "dashcam", "cam_front", "cam_garage"
  name  TEXT NOT NULL,           -- human-friendly name
  rtsp_url TEXT
  -- (optional later: rtsp_url, notes, etc.)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cameras_key ON cameras(key);

----------------------------------------------------------------------
-- Per-camera ring state (replaces global counters.segment_index/etc)
-- This is where segment_index, segment_generation, absolute_segments live now.
----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS camera_state (
  camera_id          INTEGER NOT NULL,
  sink_id            INTEGER NOT NULL,
  segment_index      INTEGER NOT NULL,
  segment_generation INTEGER NOT NULL,
  absolute_segments  INTEGER NOT NULL,
  PRIMARY KEY (camera_id, sink_id),
  FOREIGN KEY(camera_id) REFERENCES cameras(id) ON DELETE CASCADE
);


----------------------------------------------------------------------
-- Generic counters (KV) for other global integer settings if needed.
-- This replaces your old counters table in spirit, but without
-- special segment_* meanings. You can use or ignore this as you like.
----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS counters (
  name  TEXT PRIMARY KEY,
  value INTEGER NOT NULL
);

----------------------------------------------------------------------
-- Trips: now per-camera.
-- Same semantics as your original trips table, but with camera_id.
----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS trips (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  camera_id          INTEGER NOT NULL,  -- NEW: tie trip to a camera

  boot_id            TEXT    NOT NULL,
  start_time_utc     INTEGER NOT NULL,
  end_time_utc       INTEGER,           -- NULL while open
  start_segment      INTEGER NOT NULL,   -- ring index (inclusive)
  final_segment      INTEGER,            -- ring index (inclusive); NULL while open
  start_clock_source TEXT,
  end_clock_source   TEXT,
  note               TEXT,

  -- ring rollover bookkeeping
  start_gen          INTEGER NOT NULL DEFAULT 0,
  end_gen            INTEGER,            -- NULL while open

  -- Set when all files of the trip are certainly overwritten by the ring
  fully_evicted      INTEGER NOT NULL DEFAULT 0,  -- 0=false, 1=true
  evicted_at_utc     INTEGER,

  FOREIGN KEY(camera_id) REFERENCES cameras(id) ON DELETE CASCADE
);

-- Helpful indexes for per-camera queries
CREATE INDEX IF NOT EXISTS idx_trips_camera_open
  ON trips(camera_id, final_segment);

CREATE INDEX IF NOT EXISTS idx_trips_camera_range
  ON trips(camera_id, start_segment, final_segment);

CREATE INDEX IF NOT EXISTS idx_trips_camera_evicted
  ON trips(camera_id, fully_evicted, end_gen, final_segment);

----------------------------------------------------------------------
-- Saved-trip bookkeeping (same as before, but trips now have camera_id)
----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS saved_trips (
  trip_id      INTEGER PRIMARY KEY,
  saved_dir    TEXT    NOT NULL,
  saved_at_utc INTEGER NOT NULL,
  mp4_path     TEXT,
  FOREIGN KEY(trip_id) REFERENCES trips(id) ON DELETE CASCADE
);

----------------------------------------------------------------------
-- Optional: segment catalog (for NVR richness).
-- You don't HAVE to wire this up immediately for TSFilePipelineSink;
-- you can start by just using camera_state + trips like today.
-- But it's here so you can grow into per-segment metadata later.
----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS segments (
  id              INTEGER PRIMARY KEY,
  camera_id       INTEGER NOT NULL,
  segment_index   INTEGER NOT NULL,    -- ring index at creation
  segment_gen     INTEGER NOT NULL,    -- generation at creation
  absolute_index  INTEGER NOT NULL,    -- copy of camera_state.absolute_segments
  start_utc       INTEGER NOT NULL,
  end_utc         INTEGER NOT NULL,
  rel_path        TEXT NOT NULL,       -- path relative to recording root
  codec           TEXT,                -- "H264", "H265", etc.
  width           INTEGER,
  height          INTEGER,
  fps             REAL,
  bytes           INTEGER,

  FOREIGN KEY(camera_id) REFERENCES cameras(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_segments_camera_time
  ON segments(camera_id, start_utc, end_utc);

CREATE INDEX IF NOT EXISTS idx_segments_camera_abs
  ON segments(camera_id, absolute_index);
