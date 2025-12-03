Perfect, this is a great moment to “cut over clean” instead of dragging the old schema around.

Below is:

1. **Fresh schema definition** (single DB, multi-camera ready, dashcam = 1 camera).
2. **Updated checklist** tailored to “new schema first, then fix TSFilePipelineSink, then cam_service.”

---

## 1. New schema (full redefine)

Assumption: you’re fine blowing away the old DB or creating a new file. No `ALTER TABLE`, just `CREATE`.

```sql
-- Pragmas: same spirit as before
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
  name  TEXT NOT NULL           -- human-friendly name
  -- (optional later: rtsp_url, notes, etc.)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cameras_key ON cameras(key);

----------------------------------------------------------------------
-- Per-camera ring state (replaces global counters.segment_index/etc)
-- This is where segment_index, segment_generation, absolute_segments live now.
----------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS camera_state (
  camera_id          INTEGER PRIMARY KEY,
  segment_index      INTEGER NOT NULL,  -- current ring index (0..N-1)
  segment_generation INTEGER NOT NULL,  -- how many times ring wrapped
  absolute_segments  INTEGER NOT NULL,  -- total segments ever finalized
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
```

### Dashcam build initial seeding

When you first create the DB for your current dashcam build, you’ll want to seed one camera & its camera_state:

```sql
INSERT INTO cameras (key, name)
VALUES ('dashcam', 'Dashcam Front')
ON CONFLICT(key) DO NOTHING;

INSERT INTO camera_state (camera_id, segment_index, segment_generation, absolute_segments)
VALUES (
  (SELECT id FROM cameras WHERE key='dashcam'),
  0, 0, 0
)
ON CONFLICT(camera_id) DO NOTHING;
```

Your Rust `DashcamDb::setup()` can run that after creating tables.

---

# 2. Updated checklist (new-schema-first)

Reordered and tweaked to match your “drop old schema, keep TSFilePipelineSink working before touching cam_service”.

### Step 0 – New branch

* Create a git branch, e.g. `feature/new-nvr-schema`.

### Step 1 – Replace schema

* Replace your old schema SQL with the **new schema above** in `schema.sql` (or equivalent).
* Update `DashcamDb::setup()` to:

  * open connection
  * run the full schema SQL
  * run the “seed dashcam camera + camera_state” inserts.

At this point, nothing else compiles against it yet; you’re just defining the new world.

### Step 2 – Update `db.rs` API to the new schema (single camera)

Before touching `TSFilePipelineSink` or `CamService`, refit your DB layer:

* Change the DB API so that:

  * Functions that previously used `counters.segment_index`, `segment_generation`, `absolute_segments` now operate on **`camera_state`** instead.
  * Hardcode `camera_key = "dashcam"` for now (or store it in config) and resolve to `camera_id` inside `DashcamDb`.

Concretely:

* `fn get_current_segment_state(&self) -> Result<(segment_index, segment_generation, absolute_segments)>`

  * SELECT from `camera_state` WHERE camera_id = (SELECT id FROM cameras WHERE key='dashcam')
* `fn bump_segment_counters(&self, new_index: i64) -> Result<()>`

  * UPDATE `camera_state` with new `segment_index`, maybe increment `segment_generation`, `absolute_segments`, etc.
* `fn new_trip(...)`:

  * Insert into `trips` with `camera_id = dashcam_id`.

Do **not** change `TSFilePipelineSink` yet—just make `db.rs` compile and tests pass using the new schema but same behavior.

### Step 3 – Rewire `TSFilePipelineSink` to use the updated DB API

Now adjust `TsFilePipelineSink` so it works with the new DB layer:

* The GStreamer callback should **not** know about schema details; it should still:

  * increment its in-memory `segment_index`
  * send a `DbCommand` (or call into `DashcamDb`) to persist the new values.
* Internally, the DB code uses `camera_state` + `trips` as per the new schema.

Goal of this step:

* All segment indexing / trip logic works exactly like before,

  * but is now backed by `cameras` + `camera_state` + `trips` instead of the old `counters` + `trips`.

You can ignore the `segments` table for now or, if you feel like it, start inserting minimal rows there too.

### Step 4 – Confirm dashcam build behavior unchanged

* Run your existing dashcam binary (single-camera, libcamera source, TsFilePipelineSink).
* Check:

  * DB is created with the new schema.
  * `camera_state` is updated as you record.
  * `trips` behave as before.
  * Files on disk/segment naming still work as expected.

At this point, the refactor is effectively invisible from the outside, but the DB is future-proofed for multiple cameras.

### Step 5 – Only then: evolve `CamService` into multi-pipeline orchestrator

Once single-camera dashcam is stable on the new schema, you can move on to:

* Introduce config-based cameras (TOML `[[cameras]]` etc.).
* Change `CamService` to own `Vec<RecordingPipeline>` instead of a single one.
* Add RTSP sources / NVR sinks for PoE cams.
* Update DB APIs and `TSFilePipelineSink` / future NVR sinks to take a `camera_key` (or `camera_id`) instead of assuming `"dashcam"`.

---

If you want next, we can zoom into **Step 2** and sketch the new `DashcamDb` methods that wrap `camera_state` (get/set segment index, generation, absolute), so you can drop them straight into `db.rs` with minimal guesswork.
