use std::{os::unix::thread, sync::{Arc, mpsc::Receiver}, thread::{JoinHandle, Thread}};
use anyhow::Result;
use tracing::{error, info, trace};

use crate::db;

pub struct DBWorker {
    pub recvr: Receiver<i64>,
    pub dbconn: db::DashcamDb
}

impl DBWorker {
    pub fn new(recvr: Receiver<i64>) -> Result<Self> {
        let dbconn = db::DashcamDb::setup()?;

        Ok(DBWorker {
            recvr: recvr,
            dbconn: dbconn
        })
    }
}

pub fn start_db_worker(dbworker: DBWorker) -> JoinHandle<()> {
    
    let thread = std::thread::spawn(move || {

        loop {
            match dbworker.recvr.recv() {
                Ok(segment_index) => {
                    trace!("DB Worker received segment_index={}", segment_index);

                    if let Err(e) = dbworker
                        .dbconn
                        .update_segment_counters_from_index(segment_index)
                    {
                        error!("DB Worker failed to update segment counters: {:#}", e);
                    }
                }
                Err(err) => {
                    trace!("DB Worker channel closed: {}. Exiting worker thread.", err);
                    break;
                }
        }
}


    });

    thread
}