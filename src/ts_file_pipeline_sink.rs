use gstreamer as gst;
use gstreamer::prelude::*;
use anyhow::{Result, Context};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use crate::recording_pipeline::{PipelineSink, RecordingConfig, RecordingPipeline};

const MAX_SEGMENTS: i32 = 86400;
const SEGMENT_INDEX_FILE: &str = "segment_index.txt";

pub struct TsFilePipelineSink {
    // context: Arc<Mutex<RecordingPipeline>>,
    config: RecordingConfig,
    segment_index: Arc<Mutex<i32>>,
    max_segments: i32,
    make_playlist: bool,
    queue: Option<gst::Element>,
    muxer: Option<gst::Element>,
    sink: Option<gst::Element>,
    // create DB handler struct that encapsulates db connection 
    // with db methods and add as property here
}

impl TsFilePipelineSink {
    pub fn new(config:RecordingConfig, make_playlist: bool) -> Self {
        Self::new_with_max_segments(config, make_playlist, MAX_SEGMENTS)
    }

    pub fn new_with_max_segments(
        config:RecordingConfig,
        make_playlist: bool,
        max_segments: i32,
    ) -> Self {
        /// 
        let segment_index = Self::load_current_segment_index_from_disk(&config, max_segments);
        TsFilePipelineSink {
            config,
            segment_index: Arc::new(Mutex::new(segment_index)),
            max_segments,
            make_playlist,
            queue: None,
            muxer: None,
            sink: None
        }
    }

    // Convert load and save segment index to disk into funcs that write over a pipe/channel
    // and read/update sqlite database with segment_index
    fn save_segment_index_to_disk(
        config: &RecordingConfig,
        segment_index: i32,
    ) -> Result<()> {
        let path = PathBuf::from(&config.recording_dir).join(SEGMENT_INDEX_FILE);
        
        let mut file = File::create(path)
            .context("Failed to create segment index file")?;
        
        write!(file, "{}", segment_index)
            .context("Failed to write segment index")?;
        
        Ok(())
    }

    fn load_current_segment_index_from_disk(
        config: &RecordingConfig,
        max_segments: i32,
    ) -> i32 {
        let path = PathBuf::from(&config.recording_dir).join(SEGMENT_INDEX_FILE);
        
        match File::open(&path) {
            Ok(mut file) => {
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_ok() {
                    if let Ok(value) = contents.trim().parse::<i32>() {
                        if value >= 0 && value < max_segments {
                            println!("Loaded segment index: {}", value);
                            return value;
                        } else {
                            eprintln!("Warning: Invalid segment index file contents. Defaulting to 0.");
                        }
                    }
                }
            }
            Err(_) => {
                println!("Segment index file not found. Starting from 0.");
            }
        }
        
        0 // Default value
    }
}

////////////////////////////////////////////
impl PipelineSink for TsFilePipelineSink {
    fn get_sink_pad(&self) -> Result<gst::Pad> {
        self.queue.as_ref()
            .context("Queue element not initialized")?
            .static_pad("sink")
            .context("Failed to get sink pad from queue")
    }

    fn get_sink_element(&self) -> Result<gst::Element> {
        self.sink.clone()
            .context("Sink element not initialized")
    }

    fn setup_sink(&mut self, pipeline: &gst::Pipeline) -> Result<()> {
        
        let video_duration = self.config.video_duration;
        
        self.queue = Some(gst::ElementFactory::make("queue")
            .name("file_sink_queue")
            .build()
            .context("Failed to create queue")?);
        
        self.muxer = Some(gst::ElementFactory::make("mpegtsmux")
            .name("muxer")
            .build()
            .context("Failed to create mpegtsmux")?);
        
        self.sink = Some(gst::ElementFactory::make("splitmuxsink")
            .name("sink")
            .build()
            .context("Failed to create splitmuxsink")?);

        let queue = self.queue.clone().unwrap();
        let muxer = self.muxer.clone().unwrap();
        let sink = self.sink.clone().unwrap();
        
        sink.set_property("muxer", &muxer);
        sink.set_property("max-size-time", (video_duration * 1_000_000_000u64));

        let config = self.config.clone();
        let segment_index = self.segment_index.clone();
        let max_segments = self.max_segments;
        let make_playlist = self.make_playlist;
        
        sink.connect("format-location", false, move |_args| {
            let filename = if make_playlist {
                make_filename_with_playlist_closure(&config, &segment_index, max_segments)
            } else {
                make_filename_closure(&config, &segment_index, max_segments)
            };
            Some(filename.to_value())
        });

        pipeline.add_many(&[&queue, &sink])
            .context("Failed to add sink elements to pipeline")?;

        queue.link(&sink)
            .context("Failed to link queue to splitmuxsink")?;

        Ok(())
    }

}

impl Drop for TsFilePipelineSink {
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

// MAKE NEW FILENAMES for format-location gstreamer signal
fn make_filename_closure(
    config: &RecordingConfig,
    segment_index: &Arc<Mutex<i32>>,
    max_segments: i32,
) -> String {
    let mut index = segment_index.lock().unwrap();
    let current_index = *index;
    
    drop(index);


    // A call to save_segment_index_to_disk may take as long as a db call to update current segment index or something
    let _ = TsFilePipelineSink::save_segment_index_to_disk(config, current_index);
    // or send over pipe or channel to the db service to handle
    
    let subdir = {
        let subdir_digits = current_index / 1000;
        PathBuf::from(&config.recording_dir).join(subdir_digits.to_string())
    };
    
    let _ = fs::create_dir_all(&subdir);
    
    let ts_filename = format!("output_{}.ts", current_index);
    let ts_filepath = PathBuf::from(&subdir).join(&ts_filename);
    let ts_filepath_str = ts_filepath.to_string_lossy().to_string();
    
    let mut index = segment_index.lock().unwrap();
    *index += 1;
    if *index >= max_segments {
        *index = 0;
    }
    ts_filepath_str
}


fn make_filename_with_playlist_closure(
    config: &RecordingConfig,
    segment_index: &Arc<Mutex<i32>>,
    max_segments: i32,
) -> String {
    let ts_filepath = make_filename_closure(config, segment_index, max_segments);
    let ts_path = Path::new(&ts_filepath);
    
    let ts_filename = ts_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output.ts");
    
    let subdir = ts_path.parent().unwrap();
    
    let subdir_name = subdir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
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
        video_duration,
        video_duration,
        subdir_name,
        ts_filename
    );
    
    let _ = fs::write(&m3u8_filepath, playlist_content);
    
    ts_filepath
}