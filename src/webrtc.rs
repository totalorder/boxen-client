use gst::prelude::*;
use super::gstreamer_utils::Gstreamer;
use super::signalling::SignallingConnection;
use std::sync::{Arc, Weak};
use std::error::Error;
use crate::utils::{serr, StringError};
use futures::{SinkExt, StreamExt};
use serde_derive::{Deserialize, Serialize};
use gst::glib::{SignalHandlerId, BoolError};
use async_std::task;
use std::fmt;
use callback_future::CallbackFuture;
use manual_future::ManualFuture;
use promising_future;
use futures::channel;
use futures::FutureExt;
use std::panic::resume_unwind;
use std::result::Result;
use futures::future::{Future, BoxFuture, LocalBoxFuture};
use std::task::{Context, Poll};
use std::pin::Pin;
use gst::{StructureRef, PromiseError, Element};
use async_trait::async_trait;
use gst_webrtc::WebRTCSessionDescription;

// JSON messages we communicate with
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum JsonMsg {
    Ice {
        candidate: String,
        #[serde(rename = "sdpMLineIndex")]
        sdp_mline_index: u32,
    },
    Sdp {
        #[serde(rename = "type")]
        type_: String,
        sdp: String,
    },
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

#[derive(Debug, Clone)]
struct WeakWebRTC {
    weak: Weak<InnerWebRTC>
}

pub struct WebRTC {
    inner: Arc<InnerWebRTC>
}

#[derive(Debug)]
struct InnerWebRTC {
    signalling_connection: SignallingConnection,
    gstreamer: Gstreamer,
}

struct CompletableFuture<T> {
    receiver: channel::mpsc::Receiver<T>,
}

impl<T: 'static + Send + Clone> CompletableFuture<T> {
    fn new() -> (CompletableFuture<T>, FutureCompleter<T>) {
        let (sender, receiver) = channel::mpsc::channel(1);

        let completable_future = CompletableFuture { receiver };
        let future_completer = FutureCompleter { sender };

        (completable_future, future_completer)
    }

    async fn run(self) -> T {
        let (result, _receiver) = self.receiver.into_future().await;
        result.unwrap()
    }
}

#[derive(Clone)]
struct FutureCompleter<T: 'static + Send + Clone> {
    sender: channel::mpsc::Sender<T>
}

impl<T: 'static + Send + Clone> FutureCompleter<T> {
    fn complete(&self, result: T) {
        let mut sender_clone = self.sender.clone();
        task::spawn(async move {
            sender_clone.send(result).await.unwrap();
        });
    }

    async fn complete_async(&self, result: T) {
        self.sender.clone().send(result).await.unwrap();
    }
}

impl WeakWebRTC {
    pub fn upgrade(&self) -> Option<WebRTC> {
        self.weak.upgrade().map(|inner| { WebRTC { inner }})
    }
}

impl WebRTC {
    pub fn new(signalling_connection: SignallingConnection, gstreamer: Gstreamer) -> WebRTC {
        WebRTC {
            inner: Arc::new(InnerWebRTC {
                signalling_connection,
                gstreamer,
            })
        }
    }

    fn downgrade(&self) -> WeakWebRTC {
        let weak = Arc::downgrade(&self.inner);
        WeakWebRTC {
            weak
        }
    }

    pub async fn start(&self) {
        let negotiation_needed_result_fut = self.on_negotiation_needed();
        self.start_pipeline().await;
        negotiation_needed_result_fut.await;
    }

    async fn start_pipeline(&self) {
        let self_weak = self.downgrade();
        self.inner.gstreamer.pipeline.call_async_future(move |pipeline| {
            let self_strong = self_weak.upgrade().unwrap();
            println!("Starting pipeline");
            // If this fails, post an error on the bus so we exit
            self_strong.inner.gstreamer.pipeline.set_state(gst::State::Playing).unwrap();
            println!("Pipeline started");
        }).await;
    }

    async fn on_negotiation_needed(&self) {
        let self_weak = self.downgrade();

        let offer = self.inner.gstreamer.webrtcbin
            .connect_async("on-negotiation-needed", false, || async move {
                println!("on-negotiation-needed callback called");

                let self_strong = self_weak.upgrade().unwrap();

                let offer = self_strong.inner.gstreamer.webrtcbin
                    .emit_async("create-offer", |reply| {
                        reply.get_value("offer")
                            .unwrap()
                            .get::<gst_webrtc::WebRTCSessionDescription>()
                            .expect("Invalid argument")
                            .unwrap()
                    }).await;

                offer
            }).await;

        self.on_offer_created(offer).await
    }

    pub async fn on_offer_created(
        &self,
        offer: WebRTCSessionDescription) {
        self.inner.gstreamer.webrtcbin
            .emit("set-local-description", &[&offer, &None::<gst::Promise>])
            .unwrap();

        println!("Sending SDP-offer to peer");

        let message = serde_json::to_string(&JsonMsg::Sdp {
            type_: "offer".to_string(),
            sdp: offer.get_sdp().as_text().unwrap(),
        }).unwrap();

        self.inner.signalling_connection.send(message).await.unwrap();
    }
}