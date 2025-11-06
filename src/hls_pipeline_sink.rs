use crate::log;
use crate::recording_pipeline::{PipelineSink, RecordingConfig, RecordingPipeline};
use anyhow::{Context, Result};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::sync::{Arc, Mutex};
use tracing::info;

pub struct HlsPipelineSink {
    config: RecordingConfig,
    queue: Option<gst::Element>,
    parser: Option<gst::Element>,
    mux: Option<gst::Element>,
    sink: Option<gst::Element>,
    tee_pad: Option<gst::Pad>,
    webroot: String,
}

impl HlsPipelineSink {
    pub fn new(config: RecordingConfig) -> Self {
        HlsPipelineSink {
            config,
            queue: None,
            parser: None,
            mux: None,
            sink: None,
            tee_pad: None,
            webroot: String::new(),
        }
    }
}

impl PipelineSink for HlsPipelineSink {
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
        info!("Creating HlsPipelineSink");

        // Create elements
        self.queue = Some(
            gst::ElementFactory::make("queue")
                .name("hls_queue")
                .build()
                .context("Failed to create queue")?,
        );

        self.parser = Some(
            gst::ElementFactory::make("h264parse")
                .name("h264parse")
                .build()
                .context("Failed to create h264parse")?,
        );

        self.mux = Some(
            gst::ElementFactory::make("mpegtsmux")
                .name("hlsmux")
                .build()
                .context("Failed to create mpegtsmux")?,
        );

        self.sink = Some(
            gst::ElementFactory::make("hlssink")
                .name("hlssink")
                .build()
                .context("Failed to create hlssink")?,
        );

        let queue = self.queue.clone().unwrap();
        let parser = self.parser.clone().unwrap();
        let mux = self.mux.clone().unwrap();
        let sink = self.sink.clone().unwrap();

        // Configure parser
        // config interval -1 means no config frame and IS NECESSARY
        // cannot be set to 0
        parser.set_property("config-interval", 1i32);

        // Configure hlssink
        let (livestream_location, segment_location) = {
            // let recording_dir = self.config.recording_dir;
            (
                format!("{}/livestream.m3u8", self.config.recording_dir),
                format!("{}/segment%05d.ts", self.config.recording_dir),
            )
        };

        self.webroot = "/recordings/".to_string();
        #[cfg(debug_assertions)]
        {
            self.webroot = format!("{}:8888", self.webroot);
        }

        sink.set_property("playlist-location", &livestream_location);
        sink.set_property("location", &segment_location);
        sink.set_property("target-duration", 1u32);
        sink.set_property("playlist-length", 2u32);
        sink.set_property("max-files", 2u32);
        sink.set_property("playlist-root", &self.webroot);

        // Add elements to pipeline
        pipeline
            .add_many(&[&queue, &parser, &mux, &sink])
            .context("Failed to add HLS elements to pipeline")?;

        // Link elements
        gst::Element::link_many(&[&queue, &parser, &mux, &sink])
            .context("Failed to link HLS elements")?;

        info!(
            "HLS elements setup successfully. Web root: {}",
            self.webroot
        );
        Ok(())
    }
}

impl Drop for HlsPipelineSink {
    fn drop(&mut self) {
        // Clean up tee pad if it exists
        // if let Some(ref tee_pad) = self.tee_pad {
        //     if let Ok(ctx) = self.context.lock() {
        //         if let Ok(tee) = ctx.get_source_tee() {
        //             tee.release_request_pad(tee_pad);
        //         }
        //     }
        // }
    }
}
