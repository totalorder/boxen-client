mod gstreamer_utils;
mod signalling;
mod macos_workaround;
mod utils;
mod audio_stream;
mod async_test;
mod webrtc;
mod completable_future;

use webrtc::WebRTC;

use std::fs;
use gstreamer_utils::Gstreamer;
use signalling::SignallingConnection;
use utils::serr;
use async_std::task;
use std::error::Error;
use futures::join;
use futures::select;
use futures::pin_mut;
use futures::FutureExt;
use std::panic;
use crate::webrtc::WebRTCReactor;
use futures::stream::StreamExt;

fn main() {
    macos_workaround::run(|| task::block_on(main_async()));
}


async fn main_async() {
    // async_test::bla().await;
    let server_future = task::spawn(async {
        connect(10, None).await
    });

    let client_future = task::spawn(async {
        connect(20, Some(10)).await
    });


    // server.catch_unwind().await;
    // client.catch_unwind().await;
    // panic::catch_unwind(async {
    let (server, client) = join!(server_future, client_future);
    let (server, server_reactor) = server;
    let (client, client_reactor) = client;
    let (server_stream, client_stream) = (server_reactor.stream().fuse(), client_reactor.stream().fuse());
    pin_mut!(server_stream, client_stream);

    loop {
        let result = futures::select! {
            message = server_stream.select_next_some() => println!("Server received: {}", message),
            message = client_stream.select_next_some() => println!("Client received: {}", message),
            // Once we're done, break the loop and return
            complete => break,
        };
    }

    println!("Exiting");
    // });
}

async fn connect(local_id: u32, remote_id: Option<u32>) -> (WebRTC, WebRTCReactor) {
    let (signalling_connection, signalling_connection_reactor) = SignallingConnection::new(local_id, remote_id).await;

    let (gstreamer_input, gstreamer_output) = read_gstreamer_io_config();

    let gstreamer = Gstreamer::new(&gstreamer_input, &gstreamer_output);

    let (webrtc, webrtc_reactor) = WebRTC::new(signalling_connection, signalling_connection_reactor, gstreamer);

    webrtc.start().await;
    println!("WebRTC started");

    (webrtc, webrtc_reactor)
}

fn read_gstreamer_io_config() -> (String, String) {
    let input = fs::read_to_string("input.txt")
        .unwrap_or_else(|error| {
            println!("Failed to read input.txt. Using default \"autoaudiosrc\". Error: {:?}", error);
            "autoaudiosrc".into()
        })
        .trim()
        .to_owned();

    let output = fs::read_to_string("output.txt")
        .unwrap_or_else(|error| {
            println!("Failed to read output.txt. Using default \"autoaudiosink\". Error: {:?}", error);
            "autoaudiosink".into()
        })
        .trim()
        .to_owned();
    (input, output)
}

