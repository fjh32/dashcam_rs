use anyhow::{ Result};
use gstreamer as gst;
pub trait PipelineSink: Send {
    fn setup_sink(&mut self, pipeline: &gst::Pipeline) -> Result<()>;
    fn get_sink_pad(&self) -> Result<gst::Pad>;
    fn get_sink_element(&self) -> Result<gst::Element>;
}