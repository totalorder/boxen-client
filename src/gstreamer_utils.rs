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
use gst::{StructureRef, PromiseError, Element};
use futures::channel;
use futures::FutureExt;
use futures::future::Future;
use crate::completable_future::CompletableFuture;
use async_trait::async_trait;
use async_std::task;

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
}


#[async_trait]
pub trait ObjectTypeAsyncExt: ObjectType + Send{
    async fn emit_async<F, R>(&self, signal_name: &str, reply_extractor: F) -> R where
        F: FnOnce(&StructureRef) -> R + Send;

    async fn connect_async<F, R, Fut>(&self, signal_name: &str, after: bool, callback: F) -> R where
        F: FnOnce() -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output=R> + Send + 'static,
        R: 'static + Send + Clone;
}

#[async_trait]
impl<T: ObjectType + Send + Sync> ObjectTypeAsyncExt for T {
    async fn emit_async<F, R>(&self, signal_name: &str, reply_extractor: F) -> R where
        F: FnOnce(&StructureRef) -> R + Send {
        let (promise, promise_future) = gst::Promise::new_future();
        self.emit(signal_name, &[&None::<gst::Structure>, &promise]).unwrap();
        let result = promise_future.await;
        reply_extractor(result.unwrap().unwrap())
    }

    async fn connect_async<F, R, Fut>(&self, signal_name: &str, after: bool, callback: F) -> R where
        F: FnOnce() -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output=R> + Send + 'static,
        R: 'static + Send + Clone {

        let (completable_future, completer) = CompletableFuture::<R>::new();
        self.connect(signal_name, after, move |value| {
            println!("connect_async callback called");
            let callback = callback.clone();
            let completer = completer.clone();
            task::spawn( async move {
                println!("connect_async spawned task called");
                let result = callback().await;
                completer.complete(result);
            });
            None
        }).unwrap();

        completable_future.run().await
    }
}
