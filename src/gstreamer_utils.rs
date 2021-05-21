//extern crate glib;
// #[macro_use]
// extern crate lazy_static;
use lazy_static::lazy_static;

use gst::prelude::*;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
// use glib::prelude::{Cast, ObjectType};
// use pipeline::GstPipelineExtManual;

lazy_static! {
    static ref GSTREAMER_INITIALIZED: Mutex<bool> = Mutex::from(false);
}

#[derive(Debug)]
pub struct Gstreamer {
    pub pipeline: gst::Pipeline,
    pub webrtcbin: gst::Element,
    pub volume: gst::Element
}

impl Gstreamer {
    pub fn new(input_definition: &str, output_definition: &str) -> Gstreamer {
        Gstreamer::initialize_gstreamer_library();

        // Create the GStreamer pipeline
        let pipeline = gst::parse_launch(
            &format!(
                "{}{}",
                input_definition,
                " name=audiosrc ! volume name=volume ! opusenc ! rtpopuspay pt=97 ! webrtcbin. webrtcbin name=webrtcbin")).unwrap();

        // Downcast from gst::Element to gst::Pipeline
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("Not a pipeline");

        // Get access to the webrtcbin by name
        let webrtcbin = pipeline
            .get_by_name("webrtcbin")
            .expect("Can't find webrtcbin");

        // Get access to the volume by name
        let volume = pipeline
            .get_by_name("volume")
            .expect("Can't find element by name volume");

        // TODO: Should not depend on platform
        // If physical buttons are present, mute the volume so it can be unmuted by buttons
        if cfg!(target_arch="aarch64") {
            volume.set_property_from_str("mute", "true");
        }

        Gstreamer {
            pipeline,
            webrtcbin,
            volume
        }
    }

    fn initialize_gstreamer_library() {
        let mut gstreamer_initialized = GSTREAMER_INITIALIZED.lock().unwrap();

        if !*gstreamer_initialized {
            println!("Initializing gstreamer");
            gst::init().unwrap();
            Gstreamer::check_plugins().unwrap();
            *gstreamer_initialized = true;
        }
    }

    // Check if all GStreamer plugins we require are available
    fn check_plugins() -> Result<(), String>{
        let needed = [
            "videotestsrc",
            "audiotestsrc",
            "videoconvert",
            "audioconvert",
            "autodetect",
            "opus",
            "vpx",
            "webrtc",
            "nice",
            "dtls",
            "srtp",
            "rtpmanager",
            "rtp",
            "playback",
            "videoscale",
            "audioresample",
        ];

        let registry = gst::Registry::get();
        let missing = needed
            .iter()
            .filter(|n| registry.find_plugin(n).is_none())
            .cloned()
            .collect::<Vec<_>>();

        if !missing.is_empty() {
            Err(std::format!("Missing plugins: {:?}", missing))
        } else {
            Ok(())
        }
    }

    // pub async fn emit_signal<F, R>(object: impl ObjectType, signal_name: &str, reply_extractor: F) -> Result<R, PromiseError> where
    //     F: FnOnce(&StructureRef) -> R + Send + 'static,
    //     R: Send + 'static {
    //     let (completable_future, completer) = CompletableFuture::<R, PromiseError>::new();
    //
    //     let promise = gst::Promise::with_change_func(move |reply| {
    //         match reply {
    //             Ok(struct_ref) => completer.complete(reply_extractor(struct_ref.unwrap())),
    //             Err(err) => completer.fail(err)
    //         };
    //     });
    //
    //     object.emit(signal_name, &[&None::<gst::Structure>, &promise]).unwrap().unwrap();
    //     completable_future.run().await
    // }

}


