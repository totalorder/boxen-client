mod macos_workaround;
use std::sync::{Arc, Mutex, Weak};

use rand::prelude::*;

use structopt::StructOpt;

use async_std::prelude::*;
use async_std::{io, task, future};
// use futures::channel::mpsc;
use futures::sink::{Sink, SinkExt};
use futures::stream::StreamExt;

use async_tungstenite::tungstenite;
use tungstenite::Error as WsError;
use tungstenite::Message as WsMessage;

use gst::{gst_element_error, Element};
use gst::prelude::*;

use serde_derive::{Deserialize, Serialize};

use anyhow::{anyhow, bail, Context};
use std::fs;
use boxen_gpio;
use boxen_gpio::IO;
use boxen_gpio::Led;
use std::time::Duration;
use std::thread;
// use std::sync::mpsc;
//use std::sync::mpsc::{Receiver} as SyncReceiver;
use futures::channel::mpsc::{UnboundedSender, UnboundedReceiver};
// use gst::glib::types::Type::Bool;
use crossbeam_channel;
use crossbeam_channel::{Receiver, Sender, select};
use futures::TryStreamExt;
use crate::ButtonEvent::Started;

//use crate::LedState::{Off, Yellow, Green};

const STUN_SERVER: &str = "stun://stun.l.google.com:19302";
const TURN_SERVER: &str = "turn://foo:bar@webrtc.nirbheek.in:3478";

// https://gitlab.freedesktop.org/gstreamer/gst-examples/-/tree/master/webrtc
// gst-device-monitor-1.0 Audio/Source
// https://oz9aec.net/software/gstreamer/pulseaudio-device-names
// gst-launch-1.0 -v pulsesrc device=alsa_input.pci-0000_00_1f.3.analog-stereo ! audioconvert ! audioresample ! autoaudiosink

// upgrade weak reference or return
#[macro_export]
macro_rules! upgrade_weak {
    ($x:ident, $r:expr) => {{
        match $x.upgrade() {
            Some(o) => o,
            None => return $r,
        }
    }};
    ($x:ident) => {
        upgrade_weak!($x, ())
    };
}

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(short, long, default_value = "ws://boxen.deadlock.se:8443")]
    server: String,
    #[structopt(short, long)]
    peer_id: Option<u32>,
    #[structopt(short, long)]
    id: Option<u32>,
}

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

// Strong reference to our application state
#[derive(Debug, Clone)]
struct App(Arc<AppInner>);

// Weak reference to our application state. This passed into callback
#[derive(Debug, Clone)]
struct AppWeak(Weak<AppInner>);

// Actual application state
#[derive(Debug)]
struct AppInner {
    args: Args,
    pipeline: gst::Pipeline,
    webrtcbin: gst::Element,
    send_msg_tx: Mutex<futures::channel::mpsc::UnboundedSender<WsMessage>>,
    led_controller: LedController,
    button_controller: ButtonController
}

// To be able to access the AppInner's fields on the App
impl std::ops::Deref for App {
    type Target = AppInner;

    fn deref(&self) -> &AppInner {
        &self.0
    }
}

impl AppWeak {
    // Try upgrading a weak reference to a strong one
    fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

impl App {
    // Downgrade the strong reference to a weak reference
    fn downgrade(&self) -> AppWeak {
        AppWeak(Arc::downgrade(&self.0))
    }

    fn new(args: Args, peer_id: Option<u32>, gpio: GPIO) -> Result<
            (Self, impl Stream<Item = gst::Message>, impl Stream<Item = WsMessage>),
            anyhow::Error> {

        // setup_gpio();
        let input = fs::read_to_string("input.txt")
            .expect("Something went wrong reading the file")
            .trim()
            .to_owned();

        // Create the GStreamer pipeline
        let pipeline = gst::parse_launch(
            &format!(
                "{}{}",
                input,
                // "videotestsrc pattern=ball is-live=true ! vp8enc deadline=1 ! rtpvp8pay pt=96 ! webrtcbin. ",
                " name=audiosrc ! volume name=volume ! opusenc ! rtpopuspay pt=97 ! webrtcbin. webrtcbin name=webrtcbin"))?;

        // Downcast from gst::Element to gst::Pipeline
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("not a pipeline");

        // Get access to the webrtcbin by name
        let webrtcbin = pipeline
            .get_by_name("webrtcbin")
            .expect("can't find webrtcbin");

        let audiosrc = pipeline
            .get_by_name("audiosrc")
            .expect("Can't find element by name audiosrc");

        let volume = pipeline
            .get_by_name("volume")
            .expect("Can't find element by name volume");
        println!("volume.name{}", volume.get_name());

        let volume_factory = volume.get_factory().expect("Couldn't get volume factory");
        println!("volume_factory.name: {}", volume_factory.get_name());
        println!("volume_factory.element_type.name: {}", volume_factory.get_element_type().name());
        println!("volume_factory.type.name: {}", volume_factory.get_type().name());

        // let mute = volume.get_property("mute").expect("Couldn't get property mute");
        // let mute_value: bool = mute.get().expect("Couldn't get mute_value").expect("No mute_value set");
        // println!("mute_value: {}", mute_value);
        //

        if cfg!(target_arch="aarch64") {
            volume.set_property_from_str("mute", "true");
        }
        //
        // let mute = volume.get_property("mute").expect("Couldn't get property mute");
        // let mute_value: bool = mute.get().expect("Couldn't get mute_value").expect("No mute_value set");
        // println!("mute_value: {}", mute_value);
        let led_controller = gpio.led_controller;
        let button_controller = gpio.button_controller;
        listen_for_button_input(gpio.button_listener, volume, led_controller.clone());

        // Set some properties on webrtcbin
        webrtcbin.set_property_from_str("stun-server", STUN_SERVER);
        webrtcbin.set_property_from_str("turn-server", TURN_SERVER);
        webrtcbin.set_property_from_str("bundle-policy", "max-bundle");

        // Create a stream for handling the GStreamer message asynchronously
        let bus = pipeline.get_bus().unwrap();
        let send_gst_msg_rx = bus.stream();

        // Channel for outgoing WebSocket messages from other threads
        let (send_ws_msg_tx, send_ws_msg_rx) = futures::channel::mpsc::unbounded::<WsMessage>();

        println!("Created pipeline");
        let app = App(Arc::new(AppInner {
            args,
            pipeline,
            webrtcbin,
            send_msg_tx: Mutex::new(send_ws_msg_tx),
            led_controller,
            button_controller
        }));

        // Connect to on-negotiation-needed to handle sending an Offer
        if peer_id.is_some() {
            let app_clone = app.downgrade();
            println!("Connecting on-negotiation-needed. \
                Will be triggered when pipeline state set to Playing");
            app.webrtcbin
                .connect("on-negotiation-needed", false, move |values| {
                    let _webrtc = values[0].get::<gst::Element>().unwrap();

                    let app = upgrade_weak!(app_clone, None);
                    if let Err(err) = app.on_negotiation_needed() {
                        gst_element_error!(
                            app.pipeline,
                            gst::LibraryError::Failed,
                            ("Failed to negotiate: {:?}", err)
                        );
                    }

                    None
                })
                .unwrap();
        }

        // Whenever there is a new ICE candidate, send it to the peer
        let app_clone = app.downgrade();
        app.webrtcbin
            .connect("on-ice-candidate", false, move |values| {
                let _webrtc = values[0].get::<gst::Element>().expect("Invalid argument");
                let mlineindex = values[1].get_some::<u32>().expect("Invalid argument");
                let candidate = values[2]
                    .get::<String>()
                    .expect("Invalid argument")
                    .unwrap();

                let app = upgrade_weak!(app_clone, None);

                if let Err(err) = app.on_ice_candidate(mlineindex, candidate) {
                    gst_element_error!(
                        app.pipeline,
                        gst::LibraryError::Failed,
                        ("Failed to send ICE candidate: {:?}", err)
                    );
                }

                None
            })
            .unwrap();

        // Whenever there is a new stream incoming from the peer, handle it
        let app_clone = app.downgrade();
        app.webrtcbin.connect_pad_added(move |_webrtc, pad| {
            let app = upgrade_weak!(app_clone);

            if let Err(err) = app.on_incoming_stream(pad) {
                gst_element_error!(
                    app.pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to handle incoming stream: {:?}", err)
                );
            }
        });

        // Asynchronously set the pipeline to Playing
        app.pipeline.call_async(|pipeline| {
            // If this fails, post an error on the bus so we exit
            if pipeline.set_state(gst::State::Playing).is_err() {
                gst_element_error!(
                    pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to set pipeline to Playing")
                );
            }
        });

        Ok((app, send_gst_msg_rx, send_ws_msg_rx))
    }

    // Handle WebSocket messages, both our own as well as WebSocket protocol messages
    fn handle_websocket_message(&self, msg: &str) -> Result<(), anyhow::Error> {
        if msg.starts_with("ERROR") {
            bail!("Got error message: {}", msg);
        }

        let json_msg: JsonMsg = serde_json::from_str(msg)?;

        match json_msg {
            JsonMsg::Sdp { type_, sdp } => self.handle_sdp(&type_, &sdp),
            JsonMsg::Ice {
                sdp_mline_index,
                candidate,
            } => self.handle_ice(sdp_mline_index, &candidate),
        }
    }

    // Handle GStreamer messages coming from the pipeline
    fn handle_pipeline_message(&self, message: &gst::Message) -> Result<(), anyhow::Error> {
        use gst::message::MessageView;

        match message.view() {
            MessageView::Error(err) => bail!(
                "Error from element {}: {} ({})",
                err.get_src()
                    .map(|s| String::from(s.get_path_string()))
                    .unwrap_or_else(|| String::from("None")),
                err.get_error(),
                err.get_debug().unwrap_or_else(|| String::from("None")),
            ),
            MessageView::Warning(warning) => {
                println!("Warning: \"{}\"", warning.get_debug().unwrap());
            }
            _ => (),
        }

        Ok(())
    }

    // Whenever webrtcbin tells us that (re-)negotiation is needed, simply ask
    // for a new offer SDP from webrtcbin without any customization and then
    // asynchronously send it to the peer via the WebSocket connection
    fn on_negotiation_needed(&self) -> Result<(), anyhow::Error> {
        println!("Starting negotiation");

        let app_clone = self.downgrade();
        let promise = gst::Promise::with_change_func(move |reply| {
            let app = upgrade_weak!(app_clone);

            if let Err(err) = app.on_offer_created(reply) {
                gst_element_error!(
                    app.pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to send SDP offer: {:?}", err)
                );
            }
        });

        println!("Asking webrtc to create-offer");
        self.webrtcbin
            .emit("create-offer", &[&None::<gst::Structure>, &promise])
            .unwrap();

        Ok(())
    }

    // Once webrtcbin has create the offer SDP for us, handle it by sending it to the peer via the
    // WebSocket connection
    fn on_offer_created(
        &self,
        reply: Result<Option<&gst::StructureRef>, gst::PromiseError>,
    ) -> Result<(), anyhow::Error> {

        let reply = match reply {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                bail!("Offer creation future got no reponse");
            }
            Err(err) => {
                bail!("Offer creation future got error reponse: {:?}", err);
            }
        };

        println!("Received full offer from webrtc");
        let offer = reply
            .get_value("offer")
            .unwrap()
            .get::<gst_webrtc::WebRTCSessionDescription>()
            .expect("Invalid argument")
            .unwrap();
        self.webrtcbin
            .emit("set-local-description", &[&offer, &None::<gst::Promise>])
            .unwrap();


        println!("Sending SDP-offer to peer");
        // println!(
        //     "Sending SDP-offer to peer: {}",
        //     offer.get_sdp().as_text().unwrap()
        // );

        let message = serde_json::to_string(&JsonMsg::Sdp {
            type_: "offer".to_string(),
            sdp: offer.get_sdp().as_text().unwrap(),
        })
        .unwrap();

        self.send_msg_tx
            .lock()
            .unwrap()
            .unbounded_send(WsMessage::Text(message))
            .with_context(|| format!("Failed to send SDP offer"))?;

        Ok(())
    }

    // Once webrtcbin has create the answer SDP for us, handle it by sending it to the peer via the
    // WebSocket connection
    fn on_answer_created(
        &self,
        reply: Result<Option<&gst::StructureRef>, gst::PromiseError>,
    ) -> Result<(), anyhow::Error> {
        let reply = match reply {
            Ok(Some(reply)) => reply,
            Ok(None) => {
                bail!("Answer creation future got no reponse");
            }
            Err(err) => {
                bail!("Answer creation future got error reponse: {:?}", err);
            }
        };

        println!("Received SDP-answer from webrtc");
        let answer = reply
            .get_value("answer")
            .unwrap()
            .get::<gst_webrtc::WebRTCSessionDescription>()
            .expect("Invalid argument")
            .unwrap();
        self.webrtcbin
            .emit("set-local-description", &[&answer, &None::<gst::Promise>])
            .unwrap();

        println!("Sending SDP-answer to peer");
        // println!(
        //     "Sending SDP-answer to peer: {}",
        //     answer.get_sdp().as_text().unwrap()
        // );

        let message = serde_json::to_string(&JsonMsg::Sdp {
            type_: "answer".to_string(),
            sdp: answer.get_sdp().as_text().unwrap(),
        })
        .unwrap();

        self.send_msg_tx
            .lock()
            .unwrap()
            .unbounded_send(WsMessage::Text(message))
            .with_context(|| format!("Failed to send SDP answer"))?;

        Ok(())
    }

    // Handle incoming SDP answers from the peer
    fn handle_sdp(&self, type_: &str, sdp: &str) -> Result<(), anyhow::Error> {
        if type_ == "answer" {
            println!("Received SDP-answer from peer");
            // print!("Received SDP-answer from peer:\n{}\n", sdp);

            let ret = gst_sdp::SDPMessage::parse_buffer(sdp.as_bytes())
                .map_err(|_| anyhow!("Failed to parse SDP answer"))?;
            let answer =
                gst_webrtc::WebRTCSessionDescription::new(gst_webrtc::WebRTCSDPType::Answer, ret);

            println!("Send SDP-answer to webrtc (set-remote-description)");
            self.webrtcbin
                .emit("set-remote-description", &[&answer, &None::<gst::Promise>])
                .unwrap();

            Ok(())
        } else if type_ == "offer" {
            println!("Received SDP-offer from peer");
            // print!("Received SDP-offer from peer:\n{}\n", sdp);

            let ret = gst_sdp::SDPMessage::parse_buffer(sdp.as_bytes())
                .map_err(|_| anyhow!("Failed to parse SDP offer"))?;

            // And then asynchronously start our pipeline and do the next steps. The
            // pipeline needs to be started before we can create an answer
            let app_clone = self.downgrade();
            self.pipeline.call_async(move |_pipeline| {
                let app = upgrade_weak!(app_clone);

                let offer = gst_webrtc::WebRTCSessionDescription::new(
                    gst_webrtc::WebRTCSDPType::Offer,
                    ret,
                );

                println!("Send SDP-offer to webrtc");
                app.0
                    .webrtcbin
                    .emit("set-remote-description", &[&offer, &None::<gst::Promise>])
                    .unwrap();

                let app_clone = app.downgrade();
                let promise = gst::Promise::with_change_func(move |reply| {
                    let app = upgrade_weak!(app_clone);
                    if let Err(err) = app.on_answer_created(reply) {
                        gst_element_error!(
                            app.pipeline,
                            gst::LibraryError::Failed,
                            ("Failed to send SDP answer: {:?}", err)
                        );
                    }
                });

                println!("Ask webrtc to create an SDP-answer to the SDP-offer");
                app.0
                    .webrtcbin
                    .emit("create-answer", &[&None::<gst::Structure>, &promise])
                    .unwrap();
            });

            Ok(())
        } else {
            bail!("Sdp type is not \"answer\" but \"{}\"", type_)
        }
    }

    // Handle incoming ICE candidates from the peer by passing them to webrtcbin
    fn handle_ice(&self, sdp_mline_index: u32, candidate: &str) -> Result<(), anyhow::Error> {
        // println!("Received ICE from peer");
        // println!("Sending ICE to webrtc");
        self.webrtcbin
            .emit("add-ice-candidate", &[&sdp_mline_index, &candidate])
            .unwrap();

        Ok(())
    }

    // Asynchronously send ICE candidates to the peer via the WebSocket connection as a JSON
    // message
    fn on_ice_candidate(&self, mlineindex: u32, candidate: String) -> Result<(), anyhow::Error> {
        // println!("Received ICE from webrtc");
        // println!("Sending ICE to peer");
        let message = serde_json::to_string(&JsonMsg::Ice {
            candidate,
            sdp_mline_index: mlineindex,
        })
        .unwrap();

        self.send_msg_tx
            .lock()
            .unwrap()
            .unbounded_send(WsMessage::Text(message))
            .with_context(|| format!("Failed to send ICE candidate"))?;

        Ok(())
    }

    // Whenever there's a new incoming, encoded stream from the peer create a new decodebin
    fn on_incoming_stream(&self, pad: &gst::Pad) -> Result<(), anyhow::Error> {
        // Early return for the source pads we're adding ourselves
        if pad.get_direction() != gst::PadDirection::Src {
            return Ok(());
        }

        println!("Received incoming decodebin stream from webrtc");
        let decodebin = gst::ElementFactory::make("decodebin", None).unwrap();
        let app_clone = self.downgrade();
        decodebin.connect_pad_added(move |_decodebin, pad| {
            let app = upgrade_weak!(app_clone);

            if let Err(err) = app.on_incoming_decodebin_stream(pad) {
                gst_element_error!(
                    app.pipeline,
                    gst::LibraryError::Failed,
                    ("Failed to handle decoded stream: {:?}", err)
                );
            }
        });

        println!("Adding incoming decodebin stream to pipeline");
        self.pipeline.add(&decodebin).unwrap();
        decodebin.sync_state_with_parent().unwrap();

        let sinkpad = decodebin.get_static_pad("sink").unwrap();
        pad.link(&sinkpad).unwrap();

        Ok(())
    }

    // Handle a newly decoded decodebin stream and depending on its type, create the relevant
    // elements or simply ignore it
    fn on_incoming_decodebin_stream(&self, pad: &gst::Pad) -> Result<(), anyhow::Error> {
        println!("Received decodebin stream from pipeline");
        let caps = pad.get_current_caps().unwrap();
        let name = caps.get_structure(0).unwrap().get_name();

        let output = fs::read_to_string("output.txt")
            .expect("Something went wrong reading the file output.txt")
            .trim()
            .to_owned();

        // let sink = if name.starts_with("video/") {
        //     gst::parse_bin_from_description(
        //         "queue ! videoconvert ! videoscale ! autovideosink",
        //         true,
        //     )?
        // } else if name.starts_with("audio/") {
        let sink = if name.starts_with("audio/") {
            gst::parse_bin_from_description(
                &format!("{}{}", "queue ! audioconvert ! audioresample ! ", output),
                true,
            )?
        } else {
            println!("Unknown pad {:?}, ignoring", pad);
            return Ok(());
        };

        println!("Adding decodebin stream as a sink to the pipeline");
        self.pipeline.add(&sink).unwrap();
        sink.sync_state_with_parent()
            .with_context(|| format!("can't start sink for stream {:?}", caps))?;

        let sinkpad = sink.get_static_pad("sink").unwrap();
        pad.link(&sinkpad)
            .with_context(|| format!("can't link sink for stream {:?}", caps))?;
        println!("Sink started and linked to pad");
        self.button_controller.clone().started();

        Ok(())
    }
}

// Make sure to shut down the pipeline when it goes out of scope
// to release any system resources
impl Drop for AppInner {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

async fn run(
    args: Args,
    ws: impl Sink<WsMessage, Error = WsError> + Stream<Item = Result<WsMessage, WsError>>,
    peer_id: Option<u32>,
    gpio: GPIO
) -> Result<(), anyhow::Error> {
    // Split the websocket into the Sink and Stream
    let (mut ws_sink, ws_stream) = ws.split();
    // Fuse the Stream, required for the select macro
    let mut ws_stream = ws_stream.fuse();

    println!("Starting app");
    // Create our application state
    let (app, send_gst_msg_rx, send_ws_msg_rx) = App::new(args, peer_id, gpio)?;

    let mut send_gst_msg_rx = send_gst_msg_rx.fuse();
    let mut send_ws_msg_rx = send_ws_msg_rx.fuse();

    println!("Starting message loop");
    // And now let's start our message loop
    loop {
        let ws_msg = futures::select! {
            // Handle the WebSocket messages here
            ws_msg = ws_stream.select_next_some() => {
                match ws_msg? {
                    WsMessage::Close(_) => {
                        println!("peer disconnected");
                        break
                    },
                    WsMessage::Ping(data) => Some(WsMessage::Pong(data)),
                    WsMessage::Pong(_) => None,
                    WsMessage::Binary(_) => None,
                    WsMessage::Text(text) => {
                        app.handle_websocket_message(&text)?;
                        None
                    },
                }
            },
            // Pass the GStreamer messages to the application control logic
            gst_msg = send_gst_msg_rx.select_next_some() => {
                app.handle_pipeline_message(&gst_msg)?;
                None
            },
            // Handle WebSocket messages we created asynchronously
            // to send them out now
            ws_msg = send_ws_msg_rx.select_next_some() => Some(ws_msg),
            // Once we're done, break the loop and return
            complete => break,
        };

        // If there's a message to send out, do so now
        if let Some(ws_msg) = ws_msg {
            ws_sink.send(ws_msg).await?;
        }
    }

    Ok(())
}

// Check if all GStreamer plugins we require are available
fn check_plugins() -> Result<(), anyhow::Error> {
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
        bail!("Missing plugins: {:?}", missing);
    } else {
        Ok(())
    }
}

async fn async_main() -> Result<(), anyhow::Error> {
    // Initialize GStreamer first
    gst::init()?;

    check_plugins()?;

    let args = Args::from_args();

    let version = format!("version: {}", gst::version_string().as_str());
    println!("gstreamer version: {}", version);

    // setup_gpio();
    let gpio = GPIO::new();
    gpio.led_controller.clone().set_yellow_blink();

    // Connect to the given server
    let (mut ws, _) = async_tungstenite::async_std::connect_async(&args.server).await?;

    println!("Connected to websocket server");

    let id_from_file = fs::read_to_string("id.txt")
        .map(|id_from_file| id_from_file.trim().to_owned());

    // Say HELLO to the server and see if it replies with HELLO
    let our_id: u32 = if let Some(id) = args.id {
        id
    } else if let Ok(id_from_file_ok) = id_from_file {
        id_from_file_ok.parse::<u32>().unwrap()
    } else {
        rand::thread_rng().gen_range(10, 10_000)
    };

    println!("Sending HELLO, registering as id {}", our_id);

    ws.send(WsMessage::Text(format!("HELLO {}", our_id)))
        .await?;

    let msg = ws
        .next()
        .await
        .ok_or_else(|| anyhow!("didn't receive anything"))??;

    if msg != WsMessage::Text("HELLO".into()) {
        bail!("server didn't say HELLO");
    }

    println!("Received HELLO");

    let peer_id = if let Some(peer_id) = args.peer_id {
        Some(peer_id)
    } else if let Ok(peer_id_from_file) = fs::read_to_string("peer-id.txt") {
        Some(peer_id_from_file
            .trim()
            .to_owned()
            .parse::<u32>()
            .unwrap())
    } else {
        None
    };

    if let Some(peer_id) = peer_id {
        loop {
            println!("Sending SESSION, joining session {}", peer_id);
            // Join the given session
            ws.send(WsMessage::Text(format!("SESSION {}", peer_id)))
                .await?;

            let next_message_future = ws.try_next();
            let next_message_with_timeout_future = future::timeout(Duration::from_secs(5), next_message_future);
            let next_message_with_timeout = next_message_with_timeout_future.await;
            match next_message_with_timeout {
                Ok(message) => {
                    let message = message.expect("Failure when receiving message when waiting for SESSION_OK");
                    let message = message.expect("Received empty message from server when waiting for SESSION_OK");

                    if message != WsMessage::Text("SESSION_OK".into()) {
                        if message == WsMessage::Text(format!("ERROR peer '{}' not found", peer_id).into()) {
                            println!("Peer {} not found. Retrying in 10 seconds...", peer_id);
                            task::sleep(Duration::from_secs(10)).await;
                            continue
                        }
                        // ERROR peer '1' not found
                        bail!("Invalid response when waiting for SESSION_OK: {:?}", message);
                    }

                    println!("Received SESSION_OK");
                    break
                },
                Err(err) => {
                    println!("Timeout waiting for SESSION_OK: {:?}. Retrying in 10 seconds...", err);
                    task::sleep(Duration::from_secs(10)).await;
                    continue
                }
            }
        }
    }

    // All good, let's run our message loop
    run(args, ws, peer_id, gpio).await
}

#[derive(Debug)]
enum LedState {
    Off,
    Yellow,
    YellowBlink,
    Green
}

#[derive(Debug,Clone)]
struct LedController {
    led_tx: Sender<LedState>
}

impl LedController {
    fn set_off(&mut self) {
        if !cfg!(target_arch="aarch64") {
            return;
        }
        self.led_tx.send(LedState::Off);
    }

    fn set_yellow(&mut self) {
        if !cfg!(target_arch="aarch64") {
            return;
        }
        self.led_tx.send(LedState::Yellow);
    }

    fn set_green(&mut self) {
        if !cfg!(target_arch="aarch64") {
            return;
        }
        self.led_tx.send(LedState::Green);
    }

    fn set_yellow_blink(&mut self) {
        if !cfg!(target_arch="aarch64") {
            return;
        }
        self.led_tx.send(LedState::YellowBlink);
    }
}

#[derive(Debug,Clone)]
struct ButtonController {
    event_tx: Sender<ButtonEvent>
}

impl ButtonController {
    fn started(&mut self) {
        self.event_tx.send(Started);
    }
}

struct ButtonListener {
    button_rx: Receiver<(u8, bool)>,
    event_rx: Receiver<ButtonEvent>,
    yellow_initial_state: bool,
    green_initial_state: bool
}

struct GPIO {
    button_listener: ButtonListener,
    led_controller: LedController,
    button_controller: ButtonController
}

impl GPIO {
    fn new() -> GPIO {
        if !cfg!(target_arch="aarch64") {
            println!("GPIO is not supported on this platform. GPIO is disabled.");
            let (led_tx, led_rx) = crossbeam_channel::unbounded();

            let led_controller = LedController {
                led_tx
            };

            let (event_tx, event_rx) = crossbeam_channel::unbounded();

            let button_controller = ButtonController {
                event_tx
            };

            let (button_tx, button_rx) = crossbeam_channel::unbounded();
            let button_listener = ButtonListener {
                button_rx,
                event_rx,
                yellow_initial_state: true,
                green_initial_state: true
            };

            return GPIO {
                button_listener,
                led_controller,
                button_controller
            };
        }

        println!("Initializing GPIO...");
        let mut io = IO::create(Duration::from_millis(50));
        let mut led = io.create_led(24, 23);
        led.set_off();
        let yellow_button = io.create_button(27);
        let green_button = io.create_button(17);
        let button_rx = io.listen();

        let (led_tx, led_rx) = crossbeam_channel::unbounded();
        thread::spawn(move || {
            loop {
                for led_state in led_rx.iter() {
                    match led_state {
                        LedState::Off => { led.set_off() }
                        LedState::Yellow => { led.set_yellow() }
                        LedState::YellowBlink => { led.set_yellow_blink() }
                        LedState::Green => { led.set_green() }
                    }
                }
            }
        });
        let led_controller = LedController {
            led_tx
        };

        let (event_tx, event_rx) = crossbeam_channel::unbounded();
        let button_controller = ButtonController {
            event_tx
        };

        let button_listener = ButtonListener {
            button_rx,
            event_rx,
            yellow_initial_state: yellow_button.initial_state(),
            green_initial_state: green_button.initial_state(),
        };

        GPIO {
            button_listener,
            led_controller,
            button_controller
        }
    }
}

enum ButtonEvent {
    Started
}

fn listen_for_button_input(button_listener: ButtonListener, volume: Element, mut led_controller: LedController) {
    if !cfg!(target_arch="aarch64") {
        return;
    }

    thread::spawn(move || {
        println!("Listening for button input...");
        let mut started = false;
        let mut previous_state = button_listener.yellow_initial_state || button_listener.green_initial_state;
        let button_rx = button_listener.button_rx;
        let event_rx = button_listener.event_rx;
        loop {
            select! {
                recv(button_rx) -> result => {
                    let (pin, pressed) = result.expect("Received error or channel button_rx");
                    println!("Button {} pressed: {}", pin, pressed);
                    if pressed != previous_state {
                        previous_state = pressed;
                        if (started) {
                            set_mute(&volume, &mut led_controller, pressed);
                        }
                    }
                },
                recv(event_rx) -> result => {
                    let event = result.unwrap();
                    match event {
                        Started => {
                            started = true;
                            set_mute(&volume, &mut led_controller, previous_state);
                        },
                        _ => ()
                    }
                }
            }
        }
    });
}

fn set_mute(volume: &Element, led_controller: &mut LedController, unmute: bool) {
    volume.set_property_from_str("mute", if unmute { "false" } else { "true" });
    let mute = volume.get_property("mute").expect("Couldn't get property mute");
    let mute_value: bool = mute.get().expect("Couldn't get mute_value").expect("No mute_value set");
    println!("mute_value: {}", mute_value);
    if mute_value {
        led_controller.set_yellow();
    } else {
        led_controller.set_green();
    }
}

fn main() -> Result<(), anyhow::Error> {
    macos_workaround::run(|| task::block_on(async_main()))
}
