use anyhow::Result;
use std::{
    sync::mpsc::{self, Receiver},
    thread::JoinHandle,
};
use tracing::{error, info, trace};

use crate::db::{self, Trip};

pub enum DBMessage {
    SegmentUpdate(i64),
    GetSegmentIndex{
        reply: mpsc::Sender<i64>
    },
    CreateNewTrip,
    GetMostRecentTrip {
        reply: mpsc::Sender<anyhow::Result<Trip>>
    }
}

pub struct DBWorker {
    pub recvr: Receiver<DBMessage>,
    pub dbconn: db::DashcamDb,
}

impl DBWorker {
    pub fn new(recvr: Receiver<DBMessage>) -> Result<Self> {
        let dbconn = db::DashcamDb::setup()?;

        Ok(DBWorker {
            recvr: recvr,
            dbconn: dbconn,
        })
    }
}

pub fn start_db_worker(dbworker: DBWorker) -> JoinHandle<()> {
    let thread = std::thread::spawn(move || {
        while let Ok(db_message) = dbworker.recvr.recv() {
            match db_message {
                DBMessage::SegmentUpdate(segment_index) => {
                    trace!("DB Worker received segment_index={}", segment_index);

                    if let Err(e) = dbworker
                        .dbconn
                        .update_segment_counters_from_index(segment_index)
                    {
                        error!("DB Worker failed to update segment counters: {:#}", e);
                    }
                },
                DBMessage::GetSegmentIndex { reply } => {
                    let segment_index = match dbworker.dbconn.get_segment_index() {
                        Ok(val) => val,
                        Err(_) => 0,
                    };
                    let _ = reply.send(segment_index);
                },
                DBMessage::CreateNewTrip => {
                    let _ = dbworker.dbconn.new_trip();
                },
                DBMessage::GetMostRecentTrip { reply } => {
                    info!("TODO implement")
                }
            }
        }
        trace!("DB Worker channel closed. Exiting DB worker thread.");
    });

    thread
}
