use crate::recording_pipeline::{PipelineSink, RecordingConfig};
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::fs::{self};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::thread::JoinHandle;
use crate::db::db::{DashcamDb };
use crate::db::db_worker::{DBMessage,DBWorker,start_db_worker};

pub struct TsFilePipelineSink {
    config: RecordingConfig,
    db_worker_handle: Option<JoinHandle<()>>,
    db_sender: Arc<Sender<DBMessage>>,
    camera_id: i64,
    sink_id: i64,
    segment_index: Arc<AtomicI64>,
    max_segments: i64,
    queue: Option<gst::Element>,
    muxer: Option<gst::Element>,
    sink: Option<gst::Element>,
}

impl TsFilePipelineSink {
    pub fn new(config: RecordingConfig, camera_id: i64, sink_id: i64, max_segments: i64, db_sender: Arc<Sender<DBMessage>>) -> Result<Self> {
        //
        let (reply_tx, reply_rx) = mpsc::channel();
        db_sender.send(DBMessage::GetSegmentIndex { camera_id: camera_id, sink_id, reply: reply_tx })?;
        let segment_index = reply_rx.recv()?;
        //

        Ok(TsFilePipelineSink {
            config,
            db_worker_handle: None,
            db_sender: db_sender,
            camera_id,
            sink_id,
            segment_index: Arc::new(AtomicI64::new(segment_index)),
            max_segments,
            queue: None,
            muxer: None,
            sink: None,
        })
    }
}

////////////////////////////////////////////
impl PipelineSink for TsFilePipelineSink {
    fn get_sink_pad(&self) -> Result<gst::Pad> {
        self.queue
            .as_ref()
            .context("Queue element not initialized")?
            .static_pad("sink")
            .context("Failed to get sink pad from queue")
    }

    fn get_sink_element(&self) -> Result<gst::Element> {
        self.sink.clone().context("Sink element not initialized")
    }

    fn setup_sink(&mut self, pipeline: &gst::Pipeline) -> Result<()> {
        let video_duration = self.config.video_duration;

        self.queue = Some(
            gst::ElementFactory::make("queue")
                .name("file_sink_queue")
                .build()
                .context("Failed to create queue")?,
        );

        self.muxer = Some(
            gst::ElementFactory::make("mpegtsmux")
                .name("muxer")
                .build()
                .context("Failed to create mpegtsmux")?,
        );

        self.sink = Some(
            gst::ElementFactory::make("splitmuxsink")
                .name("sink")
                .build()
                .context("Failed to create splitmuxsink")?,
        );

        let queue = self.queue.clone().unwrap();
        let muxer = self.muxer.clone().unwrap();
        let sink = self.sink.clone().unwrap();

        sink.set_property("muxer", &muxer);
        sink.set_property("max-size-time", video_duration * 1_000_000_000u64);

        let config = self.config.clone();
        let camera_id = self.camera_id;
        let sink_id = self.sink_id;
        let segment_index = self.segment_index.clone();
        let max_segments = self.max_segments;
        let db_sender = self.db_sender.clone();

        // TODO rethink this format-location callback ?
        sink.connect("format-location", false, move |_args| {
            let current_index = segment_index.load(Ordering::SeqCst);

            let filename = make_filename_closure(&config, current_index);

            // wrap next_index if necessary
            let next_index = if current_index + 1 >= max_segments {
                0
            } else {
                current_index + 1
            };

            segment_index.store(next_index, Ordering::SeqCst);
            let _ = db_sender.send(DBMessage::SegmentUpdate {
                camera_id: camera_id,
                sink_id: sink_id,
                segment_index: next_index,
                max_segments: max_segments,
            });
            
            Some(filename.to_value())
        });

        pipeline
            .add_many(&[&queue, &sink])
            .context("Failed to add sink elements to pipeline")?;

        queue
            .link(&sink)
            .context("Failed to link queue to splitmuxsink")?;

        Ok(())
    }
}

impl Drop for TsFilePipelineSink {
    fn drop(&mut self) {
        // if let Some(handle) = self.db_worker_handle.take() {
        //     let _ = handle.join();
        // }
        // don't need this because an mpsc::channel() close will produce Err on recv() and exit db worker thread loop
    }
}

fn make_filename_closure(config: &RecordingConfig, segment_index: i64) -> String {
    let current_index = segment_index;

    let subdir = {
        let subdir_digits = current_index / 1000;
        PathBuf::from(&config.recording_dir).join(subdir_digits.to_string())
    };

    let _ = fs::create_dir_all(&subdir);

    let ts_filename = format!("output_{}.ts", current_index);
    let ts_filepath = PathBuf::from(&subdir).join(&ts_filename);
    let ts_filepath_str = ts_filepath.to_string_lossy().to_string();

    ts_filepath_str
}
