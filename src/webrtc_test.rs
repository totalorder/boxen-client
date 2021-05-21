mod gstreamer_utils;
mod signalling;
mod macos_workaround;
mod utils;
mod audio_stream;
mod async_test;
mod webrtc;

use webrtc::WebRTC;

use std::fs;
use gstreamer_utils::Gstreamer;
use signalling::SignallingConnection;
use utils::serr;
use async_std::task;
use std::error::Error;
use futures::join;
use futures::FutureExt;
use std::panic;

fn main() {
    macos_workaround::run(|| task::block_on(main_async()));
}


async fn main_async() {
    // async_test::bla().await;
    let server = task::spawn(async {
        connect(10, None).await.unwrap();
    });

    let client = task::spawn(async {
        connect(20, Some(10)).await.unwrap();
    });


    // server.catch_unwind().await;
    // client.catch_unwind().await;
    // panic::catch_unwind(async {
    let (server_result, client_result) = join!(server, client);
    // });
}

async fn connect(local_id: u32, remote_id: Option<u32>) -> Result<(), Box<dyn Error>> {
    let signalling_connection = SignallingConnection::new(local_id, remote_id).await;

    let (gstreamer_input, gstreamer_output) = read_gstreamer_io_config();

    let gstreamer = Gstreamer::new(&gstreamer_input, &gstreamer_output);

    let webrtc = WebRTC::new(signalling_connection, gstreamer);

    // let negotiation_result = webrtc.on_negotiation_needed().await;
    let start_result = webrtc.start().await;

    Ok(())
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

