use anyhow::{ Result};
use gstreamer as gst;
pub trait PipelineSource: Send {
    fn setup_source(&mut self, pipeline: &gst::Pipeline) -> Result<()>;
    fn get_tee(&self) -> Result<gst::Element>;
    fn get_source_pad(&self) -> Result<gst::Pad>;
}
