use std::{fs, thread, time::Duration};

use anyhow::{Context, Result, anyhow};
#[allow(dead_code)]
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::info;

use crate::{recording_pipeline::RecordingConfig};
use super::pipeline_source::PipelineSource;

pub struct V4l2PipelineSource {
    device: String,
    config: RecordingConfig,
    source: Option<gst::Element>,
    queue: Option<gst::Element>,
    capsfilter: Option<gst::Element>,
    videoconvert: Option<gst::Element>,
    encoder: Option<gst::Element>,
    parser: Option<gst::Element>,
    tee: Option<gst::Element>,
}

impl V4l2PipelineSource {
    pub fn new(config: RecordingConfig, device: Option<String>) -> Self {
        let device = match device {
            None => "dev/video0".to_string(),
            Some(dev) => dev.clone()
        };

        V4l2PipelineSource {
            device,
            config: config,
            source: None,
            queue: None,
            capsfilter: None,
            videoconvert: None,
            encoder: None,
            parser: None,
            tee: None,
        }
    }

    fn wait_for_video_device(device_path: &str) -> Result<()> {
        let mut i = 0;
        while !fs::exists(device_path)? {
            if i >= 10 {
                return Err(anyhow!("Device not found after 10 seconds"));
            }

            info!("Waiting for {}...", device_path);
            thread::sleep(Duration::from_secs(1));
            i = i + 1;
        }

        Ok(())
    }
}

impl Default for V4l2PipelineSource {
    fn default() -> Self {
        Self::new(RecordingConfig::default(), None)
    }
}

impl PipelineSource for V4l2PipelineSource {
    fn get_source_pad(&self) -> Result<gst::Pad> {
        let tee = self.tee.as_ref().context("Tee element not initialized")?;

        tee.static_pad("src")
            .context("Failed to get static pad 'src' from tee")
    }

    fn get_tee(&self) -> Result<gst::Element> {
        self.tee.clone().context("Tee element not initialized")
    }

    fn setup_source(&mut self, pipeline: &gst::Pipeline) -> Result<()> {
        info!("Creating gstreamer v4l2 source");
        Self::wait_for_video_device(&self.device)?;

        self.source = Some(
            gst::ElementFactory::make("v4l2src")
                .name("source")
                .build()
                .context("Failed to create libcamerasrc")?,
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

        self.encoder = Some(
            gst::ElementFactory::make("x264enc")
                .name("encoder")
                .build()
                .context("Failed to create x264enc")?,
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

        let source = self.source.as_ref().unwrap();
        source.set_property_from_str("device", &self.device);

        let capsfilter = self.capsfilter.as_ref().unwrap();
        let caps = gst::Caps::builder("video/x-raw")
            .field("format", "YUY2")
            .field("width", self.config.video_width)
            .field("height", self.config.video_height)
            .field("framerate", gst::Fraction::new(self.config.frame_rate, 1))
            .build();

        capsfilter.set_property("caps", &caps);

        pipeline
            .add_many(&[
                self.source.as_ref().unwrap(),
                self.queue.as_ref().unwrap(),
                self.capsfilter.as_ref().unwrap(),
                self.videoconvert.as_ref().unwrap(),
                self.encoder.as_ref().unwrap(),
                self.parser.as_ref().unwrap(),
                self.tee.as_ref().unwrap(),
            ])
            .context("Failed to add elements to v4l2 pipeline source")?;

        gst::Element::link_many(&[
            self.source.as_ref().unwrap(),
            self.queue.as_ref().unwrap(),
            self.capsfilter.as_ref().unwrap(),
            self.videoconvert.as_ref().unwrap(),
            self.encoder.as_ref().unwrap(),
            self.parser.as_ref().unwrap(),
            self.tee.as_ref().unwrap(),
        ])
        .map_err(|_| anyhow::anyhow!("Failed to link gstreamer elements"))?;

        info!("Finished setup of gstreamer v4l2 src");

        Ok(())
    }
}
