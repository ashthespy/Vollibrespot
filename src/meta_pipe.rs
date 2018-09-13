use librespot::core::events::Event;
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::core::keymaster;
use librespot::metadata::{Album, Artist, Metadata, Track};

use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use std::sync::mpsc::{Receiver, TryRecvError};
use futures::sync::mpsc;
use futures::Future;
use std::thread;
use std::time::{Instant, Duration};

#[derive(Debug)]
struct TrackMeta {
    track: Track,
    album: Album,
    artist: Vec<Artist>,
    json: Value,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum MetaMsgs {
    kSpPlaybackNotifyBecameActive,
    kSpPlaybackNotifyBecameInactive,
    kSpDeviceActive,
    kSpDeviceInactive,
    kSpSinkActive,
    kSpSinkInactive,
    metadata { meta_json: String },
    token { token: keymaster::Token },
    position_ms { position_ms: u32 },
    volume { volume_to_mixer: u16 },
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
    pub thread_handle: Option<thread::JoinHandle<()>>,
    pub cmd_rx: mpsc::Receiver<MetaMsgs>,
}

struct MetaPipeThread {
    session: Session,
    config: MetaPipeConfig,
    event_rx: Receiver<Event>,
    udp_socket: Option<UdpSocket>,
    token_info: Option<(Instant, Duration)>,
    cmd_tx: mpsc::Sender<MetaMsgs>,
    ticker: Instant,
}

impl MetaPipe {
    pub fn new(
        config: MetaPipeConfig,
        session: Session,
        event_rx: Receiver<Event>,
    ) -> (MetaPipe) {
        let (cmd_tx, cmd_rx) = mpsc::channel(2);
        let handle = thread::spawn(move || {
            debug!("Starting new MetaPipe[{}]", session.session_id());

            let meta_thread = MetaPipeThread {
                session: session,
                config: config,
                event_rx: event_rx,
                udp_socket: None,
                token_info: None,
                cmd_tx: cmd_tx,
                ticker: Instant::now(),
            };

            meta_thread.run();
        });

        (MetaPipe {
            thread_handle: Some(handle),
            cmd_rx: cmd_rx,
        })
    }
}

impl MetaPipeThread {
    fn run(mut self) {
        self.init_socket();
        loop {
                match  self.event_rx.try_recv() {
                    Ok(event) => self.handle_event(event),
                    Err(TryRecvError::Empty) => (),
                    Err(TryRecvError::Disconnected) => return,
                }
                if let Some(token_info) = self.token_info {
                    if token_info.0.elapsed() > token_info.1 {
                        info!("API Token expired, refreshing...");
                        let client_id = option_env!("CLIENT_ID");
                        match client_id {
                            Some(c_id) => {
                                const SCOPES: &str = "streaming,user-read-playback-state,user-modify-playback-state,user-read-currently-playing,user-read-private";
                                self.request_access_token(c_id, &SCOPES);
                            }
                            None => (),
                        }
                    }
                }

                // if self.ticker.elapsed() > Duration::from_secs(5) {
                //     self.ticker = Instant::now();
                //     info!("Sending cmd");
                //     self.cmd_tx.try_send(MetaMsgs::kSpDeviceActive);
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

        match event {
            Event::Load { track_id } => {
                self.handle_track_id(track_id, None);
            },
            Event::Play {track_id , position_ms} => {
                self.handle_track_id(track_id, Some(position_ms));
            }
            Event::PlaybackStarted {track_id} => {
                self.send_meta(MetaMsgs::kSpDeviceActive.to_string());
                self.handle_track_id(track_id, None);
            },
            Event::SessionActive { .. } => {
                self.handle_session_active();
                self.send_meta(MetaMsgs::kSpPlaybackNotifyBecameActive.to_string())
            },
            Event::SessionInactive { .. } => {
                self.send_meta(MetaMsgs::kSpPlaybackNotifyBecameInactive.to_string())
            },
            Event::SinkActive { .. } => self.send_meta(MetaMsgs::kSpSinkActive.to_string()),
            Event::SinkInactive { .. } => self.send_meta(MetaMsgs::kSpSinkInactive.to_string()),
            Event::PlaybackStopped {track_id} => {
                self.handle_track_id(track_id, None);
                self.send_meta(MetaMsgs::kSpDeviceInactive.to_string());
            },
            Event::Seek { position_ms } => {
                self.send_meta(json!({ "position_ms": position_ms }).to_string())
            }
            Event::GotToken { token } => self.handle_token(token),
            Event::Volume {volume_to_mixer} => self.send_meta(json!({ "volume": f64::from(volume_to_mixer)/ f64::from(u16::max_value()) * 100.0}).to_string()),
            _ => debug!("Unhandled Event:: {:?}", event),
        }
    }

    fn handle_session_active(&self) {
        info!("SessionActive!");
        // // let url:&str = "hm://radio-apollo/v3/saved-station";
        // // let url:&str = "hm://radio-apollo/v3/stations/spotify:track:5Yn4h3upuCzhwfOffa0Liy";
        // let url:&str = "hm://radio-apollo/v3/stations/spotify:track:2J56abZOk2tv0GyePJnAYN";
        // info!("{} ->", url);
        //
        // match self.session.mercury().get(url).wait() {
        //     Ok(response) => {
        //         // info!("{:?}", response.payload);
        //         let data = String::from_utf8(response.payload[0].clone()).unwrap();
        //         let value:Value  = serde_json::from_str(&data).unwrap();
        //         info!("Response: {:?}",value.to_string());
        //     },
        //     Err(e) => info!("{:?}",e),
        // };
        // and_then(|response| {
        //     protobuf::parse_from_bytes(&response.payload[0]).unwrap()
        //     });
        // info!("{}-> {:?}", url, test);
    }

    fn handle_token(&mut self, token: keymaster::Token) {
        debug!("ApiToken::<{:?}>", token);
        self.token_info = Some((Instant::now(),
                                Duration::from_secs(token.expires_in as u64 - 120u64)));
        self.send_meta(json!({"token": token}).to_string());
    }

    fn request_access_token(&mut self, client_id: &str, scopes: &str) {
        debug!("Requesting API access token");
        let token = keymaster::get_token(&self.session, client_id, scopes).wait().unwrap();
        self.handle_token(token);
    }

    fn handle_track_id(&mut self, track_id: SpotifyId, position_ms: Option<u32>) {
        let track_metadata = self.get_metadata(track_id, position_ms);
        self.send_meta(track_metadata.json.to_string());
        self.send_meta("\r\n".to_string());
    }

    fn get_metadata(&mut self, track_id: SpotifyId, position_ms: Option<u32>) -> TrackMeta {
        let track = Track::get(&self.session, track_id).wait().unwrap();
        let album = Album::get(&self.session, track.album).wait().unwrap();
        let artists = track
            .artists
            .iter()
            .map(|artist| Artist::get(&self.session, *artist).wait().unwrap())
            .collect::<Vec<Artist>>();
        let covers = album
            .covers
            .iter()
            .map(|cover| cover.to_base16())
            .collect::<Vec<_>>();
        let artist_ids = artists
                        .iter()
                        .map(|artist| artist.id.to_base62())
                        .collect::<Vec<_>>();
        let artist_names = artists.iter().map(|artist| artist.name.clone()).collect::<Vec<String>>();
        let json = json!(
            {"metadata" : {
                "track_id": track_id.to_base62(),
                "track_name": track.name,
                "artist_id": artist_ids,
                "artist_name": artist_names,
                "album_id": album.id.to_base62(),
                "album_name": album.name,
                "duration_ms": track.duration,
                "albumartId": covers,
                "position_ms": position_ms.unwrap_or(0),
            }});

        TrackMeta {
            track: track,
            album: album,
            artist: artists,
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
