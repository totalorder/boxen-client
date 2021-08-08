//use gst::prelude::*;
use async_tungstenite::{tungstenite, WebSocketStream};
use std::error::Error;
use crate::utils::serr;
use futures::sink::{Sink, SinkExt};
use futures::stream::StreamExt;
use async_std::prelude::*;
use async_tungstenite::async_std::ConnectStream;
use futures::channel::mpsc::{self, Sender, Receiver, SendError};
use futures::prelude::*;
use std::fmt;
use std::task::{Context, Poll};
use std::pin::Pin;
use futures::stream::Stream;
use futures::prelude::stream::{IntoStream, SplitStream};
// use futures::TryStreamExt;

// use simple_error::SimpleError;
// use super::gstreamer_utils;
// use super::gstreamer_utils::Gstreamer;

// For WebSocketStream.send

// For WebSocketStream.next

const STUN_SERVER: &str = "stun://stun.l.google.com:19302";
const TURN_SERVER: &str = "turn://foo:bar@webrtc.nirbheek.in:3478";
const SIGNALLING_SERVER: &str = "ws://boxen.deadlock.se:8443";


pub struct SignallingConnection {
    web_socket_stream: WebSocketStream<ConnectStream>,
    sender: Sender<String>
}

pub struct SignallingConnectionReactor {
    receiver: Receiver<String>
}

// impl Stream for SignallingConnection {
//     type Item = String;
//
//     fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
//         self.receiver.poll_next_unpin(cx)
//     }
// }

impl std::fmt::Debug for SignallingConnection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "sender: {:?}, web_socket_stream: ?", self.sender)
    }
}

impl SignallingConnection {
    pub async fn new(local_id: u32, remote_id: Option<u32>) -> (SignallingConnection, SignallingConnectionReactor) {
        // Connect to the given server
        let (mut web_socket_stream, _) =
            async_tungstenite::async_std::connect_async(SIGNALLING_SERVER).await.unwrap();

        println!("Connected to websocket server");
        println!("Sending HELLO, registering as id {}", local_id);
        web_socket_stream.send(tungstenite::Message::Text(format!("HELLO {}", local_id)))
            .await.unwrap();
        let message = web_socket_stream
            .next()
            .await
            .unwrap()
            .unwrap();

        if message != tungstenite::Message::Text("HELLO".into()) {
            panic!("Server didn't say HELLO, it said: {}", message);
        }

        println!("Received HELLO");

        if let Some(remote_id) = remote_id {
            println!("Sending SESSION, joining session {}", remote_id);
            // Join the given session
            web_socket_stream.send(tungstenite::Message::Text(format!("SESSION {}", remote_id)))
                .await.unwrap();

            let message = web_socket_stream
                .next()
                .await
                .unwrap()
                .unwrap();

            if message != tungstenite::Message::Text("SESSION_OK".into()) {
                panic!("Server didn't say SESSION_OK, it said: {}", message);
            }

            println!("Received SESSION_OK")
        }

        let (sender, receiver) = mpsc::channel(1024);
        (SignallingConnection {
            web_socket_stream,
            sender
        }, SignallingConnectionReactor {
            receiver
        })
    }

    pub async fn send(&self, message: String) -> Result<(), SendError> {
        self.sender.clone().send(message).await
    }

    // pub fn get_sender(&self) -> Sender<String> {
    //     self.sender.clone()
    // }
}

impl SignallingConnectionReactor {
    pub fn stream(self) -> impl Stream<Item = String> {
        self.receiver
    }
}
