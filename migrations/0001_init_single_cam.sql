PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA temp_store=MEMORY;

-- Simple KV store for small counters / settings
CREATE TABLE IF NOT EXISTS counters (
  name  TEXT PRIMARY KEY,
  value INTEGER NOT NULL
);

-- Ensure required counters exist
INSERT INTO counters(name, value) VALUES ('segment_index', 0)
  ON CONFLICT(name) DO NOTHING;

-- Generation increments each time the ring wraps (0-based)
INSERT INTO counters(name, value) VALUES ('segment_generation', 0)
  ON CONFLICT(name) DO NOTHING;

-- Monotonic total segments ever finalized (absolute timeline)
INSERT INTO counters(name, value) VALUES ('absolute_segments', 0)
  ON CONFLICT(name) DO NOTHING;

-- Trips
CREATE TABLE IF NOT EXISTS trips (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  boot_id            TEXT    NOT NULL,
  start_time_utc     INTEGER NOT NULL,
  end_time_utc       INTEGER,           -- NULL while open
  start_segment      INTEGER NOT NULL,   -- ring index (inclusive)
  final_segment      INTEGER,            -- ring index (inclusive); NULL while open
  start_clock_source TEXT,
  end_clock_source   TEXT,
  note               TEXT,

  -- fields for ring rollover bookkeeping
  start_gen          INTEGER NOT NULL DEFAULT 0,
  end_gen            INTEGER,            -- NULL while open

  -- Set when all files of the trip are certainly overwritten by the ring
  fully_evicted      INTEGER NOT NULL DEFAULT 0,  -- 0=false, 1=true
  evicted_at_utc     INTEGER
);

CREATE INDEX IF NOT EXISTS idx_trips_open  ON trips(final_segment);
CREATE INDEX IF NOT EXISTS idx_trips_range ON trips(start_segment, final_segment);
CREATE INDEX IF NOT EXISTS idx_trips_evicted ON trips(fully_evicted, end_gen, final_segment);

-- Saved-trip bookkeeping (optional, for your SAVE TRIP workflow)
CREATE TABLE IF NOT EXISTS saved_trips (
  trip_id      INTEGER PRIMARY KEY,
  saved_dir    TEXT    NOT NULL,
  saved_at_utc INTEGER NOT NULL,
  mp4_path     TEXT,
  FOREIGN KEY(trip_id) REFERENCES trips(id) ON DELETE CASCADE
);
