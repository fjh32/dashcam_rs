PRAGMA journal_mode=WAL;
PRAGMA synchronous=NORMAL;
PRAGMA foreign_keys=ON;
PRAGMA temp_store=MEMORY;

CREATE TABLE IF NOT EXISTS counters (
  name TEXT PRIMARY KEY,
  value INTEGER NOT NULL
);
INSERT INTO counters(name, value)
  VALUES ('segment_index', 0)
  ON CONFLICT(name) DO NOTHING;

CREATE TABLE IF NOT EXISTS trips (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  boot_id TEXT NOT NULL,
  start_time_utc INTEGER NOT NULL,
  end_time_utc INTEGER,
  start_segment INTEGER NOT NULL,
  final_segment INTEGER,
  start_clock_source TEXT,
  end_clock_source TEXT,
  note TEXT
);
CREATE INDEX IF NOT EXISTS idx_trips_open ON trips(final_segment);
CREATE INDEX IF NOT EXISTS idx_trips_range ON trips(start_segment, final_segment);

-- For when we want per-segment rows
CREATE TABLE IF NOT EXISTS segments (
  segment_index INTEGER PRIMARY KEY,
  trip_id INTEGER,
  rel_path TEXT,
  created_time_utc INTEGER,
  duration_ms INTEGER,
  size_bytes INTEGER,
  checksum TEXT,
  FOREIGN KEY(trip_id) REFERENCES trips(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_segments_trip ON segments(trip_id);

CREATE TABLE IF NOT EXISTS saved_trips (
  trip_id INTEGER PRIMARY KEY,
  saved_dir TEXT NOT NULL,
  saved_at_utc INTEGER NOT NULL,
  mp4_path TEXT,
  FOREIGN KEY(trip_id) REFERENCES trips(id) ON DELETE CASCADE
);
