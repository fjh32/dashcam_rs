use anyhow::{Context, Result};
#[allow(dead_code)]
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::info;

use crate::recording_pipeline::{PipelineSource, RecordingConfig};

///
/// def
///
pub struct LibcameraPipelineSource {
    config: RecordingConfig,
    source: Option<gst::Element>,
    encoder: Option<gst::Element>,
    queue: Option<gst::Element>,
    capsfilter: Option<gst::Element>,
    videoconvert: Option<gst::Element>,
    videoflip: Option<gst::Element>,
    parser: Option<gst::Element>,
    tee: Option<gst::Element>,
}

///
/// impls
///
impl LibcameraPipelineSource {
    pub fn new(config: RecordingConfig) -> Self {
        LibcameraPipelineSource {
            config: config,
            source: None,
            encoder: None,
            queue: None,
            capsfilter: None,
            videoconvert: None,
            videoflip: None,
            parser: None,
            tee: None,
        }
    }
}

impl Default for LibcameraPipelineSource {
    fn default() -> Self {
        Self::new(RecordingConfig::default())
    }
}

impl PipelineSource for LibcameraPipelineSource {
    fn get_source_pad(&self) -> Result<gst::Pad> {
        let tee = self.tee.as_ref().context("Tee element not initialized")?;

        tee.static_pad("src")
            .context("Failed to get static pad 'src' from tee")
    }

    fn get_tee(&self) -> Result<gst::Element> {
        self.tee.clone().context("Tee element not initialized")
    }

    fn setup_source(&mut self, pipeline: &gst::Pipeline) -> Result<()> {
        info!("Creating gstreamer libcamera source");

        self.source = Some(
            gst::ElementFactory::make("libcamerasrc")
                .name("source")
                .build()
                .context("Failed to create libcamerasrc")?,
        );

        self.encoder = Some(
            gst::ElementFactory::make("x264enc")
                .name("encoder")
                .build()
                .context("Failed to create x264enc")?,
        );

        self.queue = Some(
            gst::ElementFactory::make("queue")
                .name("queue")
                .build()
                .context("Failed to create queue")?,
        );

        self.capsfilter = Some(
            gst::ElementFactory::make("capsfilter")
                .name("capsfilter")
                .build()
                .context("Failed to create capsfilter")?,
        );

        self.videoconvert = Some(
            gst::ElementFactory::make("videoconvert")
                .name("videoconvert")
                .build()
                .context("Failed to create videoconvert")?,
        );

        self.videoflip = Some(
            gst::ElementFactory::make("videoflip")
                .name("videoflip")
                .property_from_str("method", "rotate-180")
                .build()
                .context("Failed to create videoflip")?,
        );

        self.parser = Some(
            gst::ElementFactory::make("h264parse")
                .name("h264parser")
                .build()
                .context("Failed to create h264parse")?,
        );

        self.tee = Some(
            gst::ElementFactory::make("tee")
                .name("tee")
                .build()
                .context("Failed to create tee")?,
        );

        // Configure encoder
        let encoder = self.encoder.as_ref().unwrap();
        encoder.set_property_from_str("tune", "zerolatency"); // Use string instead of int
        encoder.set_property_from_str("speed-preset", "ultrafast"); // Use string instead of int
        encoder.set_property("bitrate", 2000u32);
        encoder.set_property("key-int-max", self.config.frame_rate as u32);

        // Configure videoflip
        // let videoflip = self.videoflip.as_ref().unwrap();
        // videoflip.set_property("method", 2u32); // rotate-180

        // Configure capsfilter
        let capsfilter = self.capsfilter.as_ref().unwrap();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "NV12")
            .field("width", self.config.video_width)
            .field("height", self.config.video_height)
            .field("framerate", gst::Fraction::new(self.config.frame_rate, 1))
            .build();

        capsfilter.set_property("caps", &caps);

        // Add all elements to pipeline
        pipeline
            .add_many(&[
                self.source.as_ref().unwrap(),
                self.queue.as_ref().unwrap(),
                self.capsfilter.as_ref().unwrap(),
                self.videoflip.as_ref().unwrap(),
                self.videoconvert.as_ref().unwrap(),
                self.encoder.as_ref().unwrap(),
                self.parser.as_ref().unwrap(),
                self.tee.as_ref().unwrap(),
            ])
            .context("Failed to add elements to pipeline")?;

        // Link all elements
        gst::Element::link_many(&[
            self.source.as_ref().unwrap(),
            self.queue.as_ref().unwrap(),
            self.capsfilter.as_ref().unwrap(),
            self.videoflip.as_ref().unwrap(),
            self.videoconvert.as_ref().unwrap(),
            self.encoder.as_ref().unwrap(),
            self.parser.as_ref().unwrap(),
            self.tee.as_ref().unwrap(),
        ])
        .map_err(|_| anyhow::anyhow!("Failed to link gstreamer elements"))?;

        info!("Finished setup of gstreamer libcamera src");

        Ok(())
    }
}
