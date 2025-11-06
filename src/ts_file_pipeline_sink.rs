use crate::db_worker::{self, DBWorker};
use crate::recording_pipeline::{PipelineSink, RecordingConfig, RecordingPipeline};
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use tracing::{error, info};

use crate::db;

const MAX_SEGMENTS: i64 = 86400;

pub struct TsFilePipelineSink {
    config: RecordingConfig,
    db_worker_handle: Option<JoinHandle<()>>,
    db_sender: Arc< Sender<i64> >,
    segment_index: Arc<AtomicI64>,
    max_segments: i64,
    make_playlist: bool,
    queue: Option<gst::Element>,
    muxer: Option<gst::Element>,
    sink: Option<gst::Element>,
}

impl TsFilePipelineSink {
    pub fn new(config: RecordingConfig, make_playlist: bool) -> Result<Self> {
        Self::new_with_max_segments(config, make_playlist, MAX_SEGMENTS)
    }

    pub fn new_with_max_segments(
        config: RecordingConfig,
        make_playlist: bool,
        max_segments: i64,
    ) -> Result<Self> {

        let (sender, recvr) = channel::<i64>();
        let db_worker = DBWorker::new(recvr)?;

        // TODO load this from DB, track it in our variable, call updates to it in db from our variable over channel
        let segment_index = match db_worker.dbconn.get_segment_index() {
            Ok(val) => val,
            Err(_) => 0
        };

        let handle = db_worker::start_db_worker(db_worker);


        Ok(TsFilePipelineSink {
            config,
            db_worker_handle: Some(handle),
            db_sender: Arc::new( sender),
            segment_index: Arc::new(AtomicI64::new(segment_index)),
            max_segments,
            make_playlist,
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
        sink.set_property("max-size-time", (video_duration * 1_000_000_000u64));

        let config = self.config.clone();
        let segment_index = self.segment_index.clone();
        let max_segments = self.max_segments;
        let make_playlist = self.make_playlist;
        let db_sender = self.db_sender.clone();

        // TODO rethink this format-location callback ?
        sink.connect("format-location", false, move |_args| {
            let current_index = segment_index.load(Ordering::SeqCst);
            let _ = db_sender.send(current_index);

            let filename = if make_playlist {
                make_filename_with_playlist_closure(&config, current_index)
            } else {
                make_filename_closure(&config, current_index)
            };

            let _ = segment_index.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                let next = current + 1;
                Some(if next >= max_segments { 0 } else { next })
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

// MAKE NEW FILENAMES for format-location gstreamer signal
fn make_filename_closure(
    config: &RecordingConfig,
    segment_index: i64
) -> String {
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

fn make_filename_with_playlist_closure(
    config: &RecordingConfig,
    segment_index: i64
) -> String {
    let ts_filepath = make_filename_closure(config, segment_index);
    let ts_path = Path::new(&ts_filepath);

    let ts_filename = ts_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.ts");

    let subdir = ts_path.parent().unwrap();

    let subdir_name = subdir.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let base_name = ts_filename.strip_suffix(".ts").unwrap_or(ts_filename);
    let m3u8_filename = format!("{}.m3u8", base_name);
    let m3u8_filepath = subdir.join(&m3u8_filename);

    let video_duration = config.video_duration;

    let playlist_content = format!(
        "#EXTM3U\n\
         #EXT-X-VERSION:3\n\
         #EXT-X-TARGETDURATION:{}\n\
         #EXT-X-MEDIA-SEQUENCE:0\n\
         #EXTINF:{}.0,\n\
         /recordings/{}/{}\n\
         #EXT-X-ENDLIST\n",
        video_duration, video_duration, subdir_name, ts_filename
    );

    let _ = fs::write(&m3u8_filepath, playlist_content);

    ts_filepath
}
