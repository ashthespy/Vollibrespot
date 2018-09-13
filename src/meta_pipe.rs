use librespot::core::events::Event;
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::metadata::{Album, Artist, Metadata, Track};

use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
//
// use futures::sync::mpsc::UnboundedReceiver;
use std::sync::mpsc::{Receiver, TryRecvError};
use futures::{Future, Stream};
use std::thread;

#[derive(Debug)]
struct TrackMeta {
    track: Track,
    album: Album,
    artist: Artist,
    json: Value,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
enum MetaMsgs {
    kSpPlaybackNotifyBecameActive,
    kSpPlaybackNotifyBecameInactive,
    kSpDeviceActive,
    kSpDeviceInactive,
    kSpSinkActive,
    kSpSinkInactive,
    metadata { meta_json: String },
    token { token: String },
    position_ms { position_ms: u32 },
}

impl MetaMsgs {
    fn to_string(&self) -> String {
        return format!("{:?}", self);
    }
}

#[derive(Clone, Debug)]
pub struct MetaPipeConfig {
    pub port: u16,
    pub version: String,
}

pub struct MetaPipe {
    thread_handle: Option<thread::JoinHandle<()>>,
}

struct MetaPipeThread {
    session: Session,
    config: MetaPipeConfig,
    event_rx: Receiver<Event>,
    udp_socket: Option<UdpSocket>,
}

// As a thread
impl MetaPipe {
    pub fn new(
        config: MetaPipeConfig,
        session: Session,
        event_rx: Receiver<Event>,
    ) -> (MetaPipe) {
        let handle = thread::spawn(move || {
            debug!("Starting new MetaPipe[{}]", session.session_id());
            let meta_thread = MetaPipeThread {
                session: session,
                config: config,
                event_rx: event_rx,
                udp_socket: None,
            };

            meta_thread.run();
        });

        (MetaPipe {
            thread_handle: Some(handle),
        })
    }
}

impl MetaPipeThread {
    fn run(mut self) {
        self.init_socket();
        loop {
                match  self.event_rx.try_recv() {
                    Ok(event) => {
                        info!("Event: {:?}", event)},
                    Err(TryRecvError::Empty) => (),
                    Err(TryRecvError::Disconnected) => return,
                }
            // let (event, event_rx) = self.event_rx.into_future().wait().unwrap();
            // self.event_rx = event_rx;
            // match event {
            //     Some(event) => self.handle_event(event),
            //     None => return,
            // }

        }
    }

    fn init_socket(&mut self) {
        // Todo switch to multicast
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), self.config.port + 1);
        self.udp_socket = Some(UdpSocket::bind(addr).expect("Error starting Metadata pipe: "));
        let ver = self.config.version.clone();
        self.send_meta(ver);
    }

    fn handle_event(&mut self, event: Event) {
        // Match an enum?
        match event {
            Event::Load { track_id } => self.handle_track_id(track_id),
            Event::SessionActive { .. } => {
                self.send_meta(MetaMsgs::kSpPlaybackNotifyBecameActive.to_string())
            }
            Event::SessionInactive { .. } => {
                self.send_meta(MetaMsgs::kSpPlaybackNotifyBecameInactive.to_string())
            }
            Event::SinkActive { .. } => self.send_meta(MetaMsgs::kSpSinkActive.to_string()),
            Event::SinkInactive { .. } => self.send_meta(MetaMsgs::kSpSinkInactive.to_string()),
            Event::Seek { position_ms } => {
                self.send_meta(json!({ "position_ms": position_ms }).to_string())
            }
            Event::GotToken { token } => {
                info!("ApiToken::<{:?}>", token);
                self.send_meta(json!({"token": { "accessToken": token}}).to_string());
            }
            _ => debug!("Event:: {:?}", event),
        }
    }

    fn handle_track_id(&mut self, track_id: SpotifyId) {
        let track_metadata = self.get_metadata(track_id);
        self.send_meta(track_metadata.json.to_string());
    }

    fn get_metadata(&mut self, track_id: SpotifyId) -> TrackMeta {
        let track = Track::get(&self.session, track_id).wait().unwrap();
        let artist = Artist::get(&self.session, track.artists[0]).wait().unwrap();
        let album = Album::get(&self.session, track.album).wait().unwrap();
        let covers = album
            .covers
            .iter()
            .map(|cover| cover.to_base16())
            .collect::<Vec<_>>();
        let json = json!(
            {"metadata" : {
                "track_id": track_id.to_base16(),
                "track_name": track.name,
                "artist_id": artist.id.to_base16(),
                "artist_name": artist.name,
                "album_id": album.id.to_base16(),
                "album_name": album.name,
                "duration_ms": track.duration,
                "albumartId": covers,
                "position_ms": 0,
            }});

        TrackMeta {
            track: track,
            album: album,
            artist: artist,
            json: json,
        }
    }

    fn send_meta(&mut self, msg: String) {
        let remote_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), self.config.port);
        self.udp_socket
            .as_ref()
            .unwrap()
            .send_to(msg.as_bytes(), remote_addr)
            .expect("Unable to send metadata");
    }
}

impl Drop for MetaPipe {
    fn drop(&mut self) {
        debug!("Shutting down MetaPipe thread ...");
        if let Some(handle) = self.thread_handle.take() {
            match handle.join() {
                Ok(_) => (),
                Err(_) => error!("MetaPipe thread panicked!"),
            }
        }
    }
}

impl Drop for MetaPipeThread {
    fn drop(&mut self) {
        debug!("drop MetaPipeThread[{}]", self.session.session_id());
    }
}
