use anyhow::Result;
use std::{
    sync::mpsc::{self, Receiver, Sender},
    thread::JoinHandle,
};
use tracing::{error, info, trace};

use crate::config::AppConfig;
use crate::db::{self, DashcamDb};

pub enum DBMessage {
    SegmentUpdate {
        camera_id: i64,
        segment_index: i64,
        max_segments: i64,
    },
    GetSegmentIndex {
        camera_id: i64,
        reply: Sender<i64>,
    },
    ClampSegmentIndex {
        camera_id: i64,
        max_segments: i64,
    },

    GetCameraIdByKey {
        camera_key: String,
        reply: Sender<Option<i64>>,
    },
}

pub struct DBWorker {
    pub recvr: Receiver<DBMessage>,
    pub dbconn: DashcamDb,
}

impl DBWorker {
    /// Construct a DBWorker using the full AppConfig.
    /// This will:
    /// - open DB
    /// - run schema
    /// - initialize cameras + camera_state from config
    pub fn new(recvr: Receiver<DBMessage>, cfg: &AppConfig) -> Result<Self> {
        let dbconn = db::DashcamDb::setup_from_config(cfg)?;

        Ok(DBWorker { recvr, dbconn })
    }
}

pub fn start_db_worker(dbworker: DBWorker) -> JoinHandle<()> {
    let thread = std::thread::spawn(move || {
        while let Ok(db_message) = dbworker.recvr.recv() {

            match db_message {

                DBMessage::SegmentUpdate {
                    camera_id,
                    segment_index,
                    max_segments,
                } => {
                    trace!(
                        "DB Worker received SegmentUpdate: camera_id={}, segment_index={}, max_segments={}",
                        camera_id,
                        segment_index,
                        max_segments
                    );

                    if let Err(e) = dbworker
                        .dbconn
                        .update_segment_counters(
                            camera_id,
                            segment_index,
                            max_segments,
                        )
                    {
                        error!("DB Worker failed to update segment counters: {:#}", e);
                    }
                },

                DBMessage::GetSegmentIndex { camera_id, reply } => {
                    let segment_index = match dbworker.dbconn.get_segment_index_by_id(camera_id) {
                        Ok(val) => val,
                        Err(e) => {
                            error!(
                                "DB Worker failed to get segment index for camera_id={}: {:#}",
                                camera_id, e
                            );
                            0
                        }
                    };
                    let _ = reply.send(segment_index);
                },

                DBMessage::ClampSegmentIndex {
                    camera_id,
                    max_segments,
                } => {
                    info!(
                        "DB Worker clamping segment_index for camera_id={} to max_segments={}",
                        camera_id, max_segments
                    );
                    if let Err(e) = dbworker
                        .dbconn
                        .clamp_segment_index_by_id(camera_id, max_segments)
                    {
                        error!("DB Worker failed to clamp segment index: {:#}", e);
                    }
                },

                DBMessage::GetCameraIdByKey { camera_key, reply } => {
                    // look up in DB, but send back Option instead of blowing up
                    let id = match dbworker.dbconn.get_camera_id_by_key(&camera_key) {
                        Ok(id) => Some(id),
                        Err(e) => {
                            error!(
                                "DB Worker failed to get camera_id for key='{}': {:#}",
                                camera_key, e
                            );
                            None
                        }
                    };
                    let _ = reply.send(id);
                }
            }

        }
        trace!("DB Worker channel closed. Exiting DB worker thread.");
    });

    thread
}
