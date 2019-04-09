use librespot::connect::spirc::Spirc;
use librespot::core::events::Event;
use librespot::core::keymaster;
use librespot::core::session::Session;
use librespot::core::spotify_id::SpotifyId;
use librespot::metadata::{Album, Artist, Metadata, Track};
use std::io::ErrorKind;

use serde_json;
use serde_json::Value;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};

use futures::Future;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug)]
struct TrackMeta {
    track: Track,
    album: Album,
    artist: Vec<Artist>,
    json: Value,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
enum VolumioMsgs {
    volHello = 0x1,
    volHeartBeat = 0x2,
    volReqToken = 0x3,
    volStop = 0x4,
}

#[derive(Debug, Serialize)]
#[allow(non_camel_case_types)]
pub enum MetaMsgs<'a> {
    kSpPlaybackNotifyBecameActive,
    kSpPlaybackNotifyBecameInactive,
    kSpDeviceActive,
    kSpDeviceInactive,
    kSpSinkActive,
    kSpSinkInactive,
    token(keymaster::Token),
    position_ms(u32),
    volume(f64),
    state { status: &'a str },
    // metadata(String),
}

impl<'a> MetaMsgs<'a> {
    fn to_string(&self) -> String {
        format!("{:?}", self)
    }
}

#[derive(Clone, Debug)]
pub struct MetaPipeConfig {
    pub port: u16,
    pub version: String,
}

pub struct MetaPipe {
    pub thread_handle: Option<thread::JoinHandle<()>>,
    task_tx: Option<Sender<Empty>>,
}

struct MetaPipeThread {
    session: Session,
    config: MetaPipeConfig,
    task_rx: Receiver<Empty>,
    event_rx: Receiver<Event>,
    udp_socket: Option<UdpSocket>,
    token_info: Option<(Instant, Duration)>,
    buf: [u8; 2],
    spirc: Arc<Spirc>,
}

const SCOPES: &str = "streaming,user-read-playback-state,user-modify-playback-state,user-read-currently-playing,user-read-private";
const CLIENT_ID: Option<&'static str> = option_env!("CLIENT_ID");

#[derive(Debug)]
enum Empty {}

impl MetaPipe {
    pub fn new(
        config: MetaPipeConfig,
        session: Session,
        event_rx: Receiver<Event>,
        spirc: Arc<Spirc>,
    ) -> (MetaPipe) {
        let (task_tx, task_rx) = channel::<Empty>();
        let handle = thread::spawn(move || {
            debug!("Starting new MetaPipe[{}]", session.session_id());

            let meta_thread = MetaPipeThread {
                session: session,
                config: config,
                task_rx: task_rx,
                event_rx: event_rx,
                udp_socket: None,
                token_info: None,
                buf: [0u8; 2],
                spirc: spirc,
            };

            meta_thread.run();
        });

        (MetaPipe {
            thread_handle: Some(handle),
            task_tx: Some(task_tx),
        })
    }
}

impl MetaPipeThread {
    fn run(mut self) {
        self.init_socket();

        loop {
            let mut got_volumio_msg = false;

            if self.session.is_invalid() {
                error!("Session no longer valid");
                break;
            }

            match self.task_rx.try_recv() {
                Ok(_empty) => (),
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => break,
            }

            match self.event_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(event) => self.handle_event(event),
                Err(RecvTimeoutError::Timeout) => (),
                Err(RecvTimeoutError::Disconnected) => {
                    error!("EventSender disconnected");
                    break;
                }
            }
            if let Some(token_info) = self.token_info {
                if token_info.0.elapsed() > token_info.1 {
                    info!("API Token expired, refreshing...");
                    self.request_access_token();
                }
            }

            if let Some(ref udp_socket) = self.udp_socket {
                match udp_socket.recv(&mut self.buf) {
                    Ok(_nbytes) => {
                        got_volumio_msg = true;
                    }
                    Err(ref err) if err.kind() != ErrorKind::WouldBlock => warn!("WouldBlock"),
                    _ => (),
                }
            }

            if got_volumio_msg {
                self.handle_volumio_msg(); // Meh pass in the message
            }
        }
        self.send_meta(&MetaMsgs::kSpSinkInactive.to_string());
    }

    fn init_socket(&mut self) {
        // Todo switch to multicast
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), self.config.port + 1);
        let soc = UdpSocket::bind(addr).expect("Error starting Metadata pipe: ");
        soc.set_nonblocking(true).expect("Error starting Metadata pipe: ");
        self.udp_socket = Some(soc);
        info!("Metadata pipe established");
        let ver = self.config.version.clone();
        self.send_meta(&ver);
    }

    fn handle_event(&mut self, event: Event) {
        debug!("{:?}", event);
        match event {
            Event::Load { track_id } => {
                self.handle_track_id(track_id, None);
            }
            Event::Play {
                track_id,
                position_ms,
            } => {
                self.send_meta(&serde_json::to_string(&MetaMsgs::state { status: "play" }).unwrap());
                self.handle_track_id(track_id, Some(position_ms));
            }
            Event::Pause {
                track_id,
                position_ms,
            } => {
                self.send_meta(&serde_json::to_string(&MetaMsgs::state { status: "pause" }).unwrap());
                self.handle_track_id(track_id, Some(position_ms));
            }
            Event::PlaybackStarted { .. } => {
                self.send_meta(&MetaMsgs::kSpDeviceActive.to_string());
                // self.handle_track_id(track_id, None);
            }
            Event::SessionActive { .. } => {
                self.handle_session_active();
                self.send_meta(&MetaMsgs::kSpPlaybackNotifyBecameActive.to_string())
            }
            Event::SessionInactive { .. } => {
                self.send_meta(&MetaMsgs::kSpPlaybackNotifyBecameInactive.to_string())
            }
            Event::SinkActive { .. } => self.send_meta(&MetaMsgs::kSpSinkActive.to_string()),
            Event::SinkInactive { .. } => self.send_meta(&MetaMsgs::kSpSinkInactive.to_string()),
            Event::PlaybackStopped { .. } => {
                // self.handle_track_id(track_id, None);
                self.send_meta(&MetaMsgs::kSpDeviceInactive.to_string());
            }
            Event::Seek { position_ms } => {
                self.send_meta(&serde_json::to_string(&MetaMsgs::position_ms(position_ms)).unwrap());
            }
            Event::GotToken { token } => self.handle_token(token),
            Event::Volume { volume_to_mixer } => {
                let pvol = f64::from(volume_to_mixer) / f64::from(u16::max_value()) * 100.0;
                debug!("Event::Volume({})", pvol);
                self.send_meta(&serde_json::to_string(&MetaMsgs::volume(pvol)).unwrap());
            }
            _ => debug!("Unhandled Event:: {:?}", event),
        }
    }

    fn handle_volumio_msg(&mut self) {
        use self::VolumioMsgs::*;
        match self.buf[0] {
            0x1 => {
                info!("{:?}", volHello);
            }
            0x2 => {
                info!("{:?}", volHeartBeat);
            }
            0x3 => {
                info!("{:?}", volReqToken);
                self.request_access_token()
            }
            0x4 => {
                info!("{:?}", volStop);
                self.spirc.pause()
            }
            _ => debug!("volumioMsg:: {:?}", self.buf[0]),
        }
    }

    fn handle_session_active(&self) {
        info!("SessionActive!");
    }

    fn handle_token(&mut self, token: keymaster::Token) {
        debug!("ApiToken::<{:?}>", token);
        self.token_info = Some((
            Instant::now(),
            Duration::from_secs(u64::from(token.expires_in) - 120u64),
        ));
        self.send_meta(&serde_json::to_string(&MetaMsgs::token(token)).unwrap());
    }

    fn request_access_token(&mut self) {
        debug!("Requesting API access token");
        match CLIENT_ID {
            Some(client_id) => {
                let token = keymaster::get_token(&self.session, client_id, SCOPES)
                    .wait()
                    .unwrap();
                self.handle_token(token);
            }
            None => warn!("Schade!"),
        }
    }

    fn handle_track_id(&mut self, track_id: SpotifyId, position_ms: Option<u32>) {
        let track_metadata = self.get_metadata(track_id, position_ms);
        self.send_meta(&track_metadata.json.to_string());
        self.send_meta(&"\r\n".to_string());
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
        let artist_names = artists
            .iter()
            .map(|artist| artist.name.clone())
            .collect::<Vec<String>>();
        let json = json!(
        { "metadata" : {
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

    fn send_meta(&mut self, msg: &str) {
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
        debug!("drop MetaPipe");
        drop(self.task_tx.take());
        if let Some(handle) = self.thread_handle.take() {
            match handle.join() {
                Ok(_) => debug!("Closed MetaPipe thread"),
                Err(_) => error!("MetaPipe panicked!"),
            }
        } else {
            warn!("Unable to drop MetaPipe");
        }
    }
}

impl Drop for MetaPipeThread {
    fn drop(&mut self) {
        debug!("drop MetaPipeThread[{}]", self.session.session_id());
    }
}
