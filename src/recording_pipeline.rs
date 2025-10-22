#[allow(dead_code)]

use anyhow::{Context, Result, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
/////////////////////// Traits ////////////////////////
pub trait PipelineSource: Send {
    fn setup_source(&mut self, pipeline: &gst::Pipeline) -> Result<()>;
    fn get_tee(&self) -> Result<gst::Element>;
    fn get_source_pad(&self) -> Result<gst::Pad>;
}

pub trait PipelineSink: Send {
    fn setup_sink(&mut self, pipeline: &gst::Pipeline) -> Result<()>;
    fn get_sink_pad(&self) -> Result<gst::Pad>;
    fn get_sink_element(&self) -> Result<gst::Element>;
}
////////////////////////////////////////////////////////////
#[derive(Clone)]
pub struct RecordingConfig {
    pub recording_dir: String,
    pub video_duration: u64, // in seconds
    pub video_width: i32,
    pub video_height: i32,
    pub frame_rate: i32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            recording_dir: "./recordings".to_string(),
            video_duration: 2, // 2 second .ts files
            video_width: 1920,
            video_height: 1080,
            frame_rate: 30,
        }
    }
}
////////////////////////////////////////////////////////////
/// Main recording pipeline that orchestrates sources and sinks
#[allow(dead_code)]
pub struct RecordingPipeline {
    config: RecordingConfig,
    pipeline: gst::Pipeline,
    pipeline_running: Arc<AtomicBool>,

    source: Option<Box<dyn PipelineSource>>,
    sinks: Vec<Box<dyn PipelineSink>>,

    pub current_video_name: Arc<Mutex<String>>,
    pipeline_thread: Option<std::thread::JoinHandle<()>>,
}

#[allow(dead_code)]
impl RecordingPipeline {
    pub fn new(config: RecordingConfig) -> Result<Self> {
        gst::init()?;

        std::fs::create_dir_all(&config.recording_dir)?;
        let pipeline = gst::Pipeline::with_name("dashcam_pipeline");
        Ok(Self {
            pipeline: pipeline,
            source: None,
            sinks: Vec::new(),
            config,
            pipeline_running: Arc::new(AtomicBool::new(false)),
            current_video_name: Arc::new(Mutex::new("None".to_string())),
            pipeline_thread: None,
        })
    }

    pub fn set_source(&mut self, source: Box<dyn PipelineSource>) {
        self.source = Some(source);
    }

    pub fn get_source_tee(&self) -> Result<gst::Element> {
        self.source.as_ref().context("No source set")?.get_tee()
    }

    pub fn add_sink(&mut self, sink: Box<dyn PipelineSink>) {
        self.sinks.push(sink);
    }

    pub fn is_running(&self) -> bool {
        self.pipeline_running.load(Ordering::SeqCst)
    }

    pub fn config(&self) -> &RecordingConfig {
        &self.config
    }

    /// Setup the GStreamer Pipeline
    /// - Setup 1 Source
    /// - Setup multiple Sinks
    /// - Connect Source Tee to each Sink via Pads
    fn build_pipeline(&mut self) -> Result<()> {
        let source = self.source.as_mut().context("No source set for pipeline")?;

        if self.sinks.is_empty() {
            anyhow::bail!("No sinks added to pipeline");
        }


        source.setup_source(&self.pipeline)?;

        println!("HEREEEE");
        for sink in &mut self.sinks {
            sink.setup_sink(&self.pipeline)?;
        }

        let source_tee = source.get_tee()?;
        for sink in &self.sinks {
            let tee_src_pad = source_tee
                .request_pad_simple("src_%u")
                .context("Failed to request pad from tee")?;
            let sink_pad = sink.get_sink_pad()?;

            tee_src_pad
                .link(&sink_pad)
                .context("Failed to link tee to sink")?;
        }

        Ok(())
    }

    pub fn start_pipeline(&mut self) -> Result<()> {
        if self.pipeline_thread.is_none() {
            println!(
                "Starting pipeline at {}",
                chrono::Local::now().format("%m-%d-%Y %H:%M:%S")
            );

            let pipeline = self.pipeline.clone();
            let pipeline_running = self.pipeline_running.clone();

            self.build_pipeline()?;
            pipeline_running.store(true, Ordering::SeqCst);

            let handle = std::thread::spawn(move || {
                Self::pipeline_runner(pipeline, pipeline_running);
            });
            self.pipeline_thread = Some(handle);

            Ok(())
        } else {
            bail!("Pipeline is already started");
        }
    }

    pub fn stop_pipeline(&mut self) -> Result<()> {
        println!("Stopping pipeline");

        if self.pipeline_running.load(Ordering::SeqCst) {
            self.pipeline_running.store(false, Ordering::SeqCst);

            self.pipeline.send_event(gst::event::Eos::new());

            if let Some(handle) = self.pipeline_thread.take() {
                let _ = handle.join();
                // self.pipeline_thread = None;
            }

            self.pipeline.set_state(gst::State::Null)?;
        }

        Ok(())
    }

    fn pipeline_runner(pipeline: gst::Pipeline, pipeline_running: Arc<AtomicBool>) {
        println!("Pipeline thread starting");

        println!("About to set pipeline state to PLAYING...");
        match pipeline.set_state(gst::State::Playing) {
            Ok(_) => println!("✅ Pipeline state successfully set to PLAYING"),
            Err(e) => {
                eprintln!("❌ Failed to start pipeline: {}", e);
                pipeline_running.store(false, Ordering::SeqCst);
                return;
            }
        }

        println!("Getting pipeline bus...");
        let bus = pipeline.bus().expect("Pipeline has no bus");
        println!("Got pipeline bus, entering message loop...");

        loop {
            if !Self::handle_bus_message(&bus) {
                break;
            }

            if !pipeline_running.load(Ordering::SeqCst) {
                println!("Running flag set to false, exiting pipeline loop");
                break;
            }
        }

        pipeline_running.store(false, Ordering::SeqCst);
        println!("Pipeline thread exiting");
    }

    fn handle_bus_message(bus: &gst::Bus) -> bool {
        use gst::MessageView;

        let msg = bus.timed_pop_filtered(
            gst::ClockTime::from_mseconds(500),
            &[
                gst::MessageType::Error,
                gst::MessageType::Element,
                gst::MessageType::Eos,
            ],
        );

        let mut continue_flag = true;

        if let Some(msg) = msg {
            match msg.view() {
                MessageView::Eos(..) => {
                    println!("End-Of-Stream reached");
                    continue_flag = false;
                }
                MessageView::Error(err) => {
                    eprintln!("Error: {} ({:?})", err.error(), err.debug());
                    continue_flag = false;
                }
                MessageView::Element(element) => {
                    if let Some(structure) = element.structure() {
                        if structure.name() == "splitmuxsink-fragment-closed" {
                            // println!("Fragment closed");
                        }
                    }
                    continue_flag = true;
                }
                _ => {
                    continue_flag = true;
                }
            }
        }
        return continue_flag;
    }
}

// like destructor
impl Drop for RecordingPipeline {
    fn drop(&mut self) {
        if self.is_running() {
            let _ = self.stop_pipeline();
        }
    }
}

/// TEST
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_creation() {
        let config: RecordingConfig = RecordingConfig::default();
        let pipeline = RecordingPipeline::new(config);
        assert!(pipeline.is_ok());
    }
}
