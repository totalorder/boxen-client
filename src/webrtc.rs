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
use gst::{StructureRef, PromiseError};
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
pub trait ObjectTypeExt: ObjectType + Send{
    async fn emit_async<F, R>(&self, signal_name: &str, reply_extractor: F) -> Result<R, PromiseError> where
        F: FnOnce(&StructureRef) -> R + Send + Clone + 'static,
        R: Send + Clone + 'static;

    async fn connect_async<F, R, E, Fut>(&self, signal_name: &str, after: bool, callback: F) -> Result<R, E> where
        F: FnOnce() -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output=Result<R, E>> + Send + 'static,
        R: 'static + Send + Clone, E: 'static + Send + Clone;
}

#[async_trait]
impl<T: ObjectType + Send + Sync> ObjectTypeExt for T {
    async fn emit_async<F, R>(&self, signal_name: &str, reply_extractor: F) -> Result<R, PromiseError> where
        F: FnOnce(&StructureRef) -> R + Send + Clone + 'static,
        R: Send + Clone + 'static {
        let (completable_future, completer) = CompletableFuture::<R, PromiseError>::new();

        let promise = gst::Promise::with_change_func(move |reply| {
            match reply {
                Ok(struct_ref) => completer.complete(Ok(reply_extractor(struct_ref.unwrap()))),
                Err(err) => completer.complete(Err(err))
            };
        });

        self.emit(signal_name, &[&None::<gst::Structure>, &promise]).unwrap();
        completable_future.run().await
    }

    async fn connect_async<F, R, E, Fut>(&self, signal_name: &str, after: bool, callback: F) -> Result<R, E> where
        F: FnOnce() -> Fut + Send + Sync + Clone + 'static,
        Fut: Future<Output=Result<R, E>> + Send + 'static,
        R: 'static + Send + Clone, E: 'static + Send + Clone {

        let (completable_future, completer) = CompletableFuture::<R, E>::new();
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


struct CompletableFuture<T, E> {
    receiver: channel::mpsc::Receiver<Result<T, E>>,
}

impl<T: 'static + Send + Clone, E: 'static + Send + Clone> CompletableFuture<T, E> {
    fn new() -> (CompletableFuture<T, E>, FutureCompleter<T, E>) {
        let (sender, receiver) = channel::mpsc::channel(1);

        let completable_future = CompletableFuture { receiver };
        let future_completer = FutureCompleter { sender };

        (completable_future, future_completer)
    }

    async fn run(self) -> Result<T, E> {
        let (result, _receiver) = self.receiver.into_future().await;
        result.unwrap()
    }
}

#[derive(Clone)]
struct FutureCompleter<T: 'static + Send + Clone, E: 'static + Send + Clone> {
    sender: channel::mpsc::Sender<Result<T, E>>
}

impl<T: 'static + Send + Clone, E: 'static + Send + Clone> FutureCompleter<T, E> {
    fn complete(&self, result: Result<T, E>) {
        let mut sender_clone = self.sender.clone();
        task::spawn(async move {
            sender_clone.send(result).await.unwrap();
        });
    }

    async fn complete_async(&self, result: Result<T, E>) {
        self.sender.clone().send(result).await.unwrap();
    }
}

impl WeakWebRTC {
    pub fn upgrade(&self) -> Option<WebRTC> {
        self.weak.upgrade().map(|inner| { WebRTC { inner }})
    }
}

async fn call_example() {
    // let f = async |x, y|{
    //     x > y
    // };
    let f = |x, y| async move {
        x > y
    };

    let z = example(f).await;
}

async fn example<F, Fut>(f: F)
    where
        F: FnOnce(i32, i32) -> Fut,
        Fut: Future<Output = bool> {
    f(1, 2).await;
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
        let (completable_future, completer) = CompletableFuture::<(), ()>::new();
        self.inner.gstreamer.pipeline.call_async(move |pipeline| {
            let self_strong = self_weak.upgrade().unwrap();
            println!("Starting pipeline");
            // If this fails, post an error on the bus so we exit
            self_strong.inner.gstreamer.pipeline.set_state(gst::State::Playing).unwrap();
            println!("Pipeline started");
            completer.complete(Ok(()));
        });
        completable_future.run().await.unwrap();
    }

    fn receive_reply(&self, reply: Result<Option<&StructureRef>, PromiseError>) {
        let r = reply;
    }


    async fn on_negotiation_needed(&self) {
        let self_weak = self.downgrade();

        let result = self.inner.gstreamer.webrtcbin
            .connect_async("on-negotiation-needed", false, || async move {
                println!("on-negotiation-needed callback called");

                let self_strong = self_weak.upgrade().unwrap();

                let signal_response = self_strong.inner.gstreamer.webrtcbin.emit_async("create-offer", |reply| {
                    reply
                        .get_value("offer")
                        .unwrap()
                        .get::<gst_webrtc::WebRTCSessionDescription>()
                        .expect("Invalid argument")
                        .unwrap()
                }).await;

                signal_response
            });
        println!("on-negotiation-needed callback registered");
        let result = result;
        println!("on-negotiation-needed awaited!");
    }

    pub async fn on_offer_created(
        &self,
        reply: Result<Option<&gst::StructureRef>, gst::PromiseError>) {

        let reply = match reply {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                panic!("Offer creation future got no response");
            }
            Err(error) => {
                panic!("Offer creation future got error response: {:?}", error);
            }
        };

        println!("Received full offer from webrtc");
        let offer = reply
            .get_value("offer")
            .unwrap()
            .get::<gst_webrtc::WebRTCSessionDescription>()
            .expect("Invalid argument")
            .unwrap();
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