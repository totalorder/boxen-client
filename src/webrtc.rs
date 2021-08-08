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
use gst_webrtc::WebRTCSessionDescription;
use crate::completable_future::CompletableFuture;
use crate::gstreamer_utils::ObjectTypeAsyncExt;
use async_std::stream::Stream;
use async_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Policy;
use futures::pin_mut;
use futures::prelude::stream::{SplitStream, Map, Then};
use futures::channel::mpsc::Receiver;
use crate::signalling::SignallingConnectionReactor;
use std::rc::Rc;

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

#[derive(Debug, Clone)]
struct WeakWebRTC {
    weak: Weak<InnerWebRTC>
}

pub struct WebRTC {
    inner: Arc<InnerWebRTC>
}

pub struct WebRTCReactor {
    webrtc: WeakWebRTC,
    signalling_connection_reactor: SignallingConnectionReactor,
}

#[derive(Debug)]
struct InnerWebRTC {
    signalling_connection: SignallingConnection,
    gstreamer: Gstreamer,
}

impl WeakWebRTC {
    pub fn upgrade(&self) -> Option<WebRTC> {
        self.weak.upgrade().map(|inner| { WebRTC { inner }})
    }
}

impl WebRTC {
    pub fn new(signalling_connection: SignallingConnection, signalling_connection_reactor: SignallingConnectionReactor, gstreamer: Gstreamer) -> (WebRTC, WebRTCReactor) {
        let webrtc = WebRTC {
            inner: Arc::new(InnerWebRTC {
                signalling_connection,
                gstreamer,
            })
        };

        let webrtc_weak = webrtc.downgrade();
        let webrtc_reactor = WebRTCReactor {
            webrtc: webrtc_weak,
            signalling_connection_reactor
        };

        (webrtc, webrtc_reactor)
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

    pub async fn on_signalling_message_received(&self, message: String) -> String {
        println!("WebRTC message_received: {}", message);
        message
    }
}

impl WebRTCReactor {
    pub fn stream(self) -> impl Stream<Item = String> {
        // let webrtc_strong = self.webrtc.upgrade().unwrap();
        let webrtc_weak = self.webrtc;
        // let weak = webrtc_weak.weak.clone();


        // let x: dyn FnMut(String) -> impl Future<Output=String> = |message: String| async move {
        //     println!("WebRTCReactor received message: {}", message);
        //     let webrtc_weak = weak.clone();
        //     // let webrtc_strong = webrtc_weak.upgrade().unwrap();
        //     // let message_copy = message.clone();
        //     // webrtc_strong.message_received(message);
        //     // "w00t".trim()
        //     message
        // };
        self.signalling_connection_reactor.stream().then(move |message| {
            WebRTCReactor::on_signalling_message_received(webrtc_weak.clone().upgrade().unwrap(), message)
        })
    }

    async fn on_signalling_message_received(webrtc: WebRTC, message: String) -> String {
        webrtc.on_signalling_message_received(message).await
    }
}

// impl Stream for WebRTC {
//     type Item = ();
//
//     fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         let signalling_poll = self.inner.signalling_connection.poll_next_unpin(cx);
//         match signalling_poll {
//             Poll::Pending => Poll::Pending,
//             Poll::Ready(message) => {
//                 message.map_or_else(|| { Poll::Ready(None) }, |message| {
//                     let mut message_received_fut = self.message_received(message);
//                     pin_mut!(message_received_fut);
//                     match message_received_fut.poll(cx) {
//                         Poll::Pending => Poll::Pending,
//                         Poll::Ready(_) => Poll::Pending
//                     }
//                 })
//             }
//         }
//
//
//         // let poll = self.inner.signalling_connection.poll_next(cx);
//
//     }
// }