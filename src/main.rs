use futures;
use getopts;
#[macro_use]
extern crate log;
use tokio_signal;
#[macro_use]
extern crate serde_json;
#[macro_use]
extern crate serde_derive;

use env_logger::{fmt, Builder};
use futures::sync::mpsc::UnboundedReceiver;
use futures::{Async, Future, Poll, Stream};

use std::env;
use std::io::{self, Write};
use std::mem;
use std::process::exit;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::Instant;

// Multi thread
// use tokio::{reactor::Handle, runtime::Runtime};

// Single thread
use tokio::runtime::{
    current_thread,
    current_thread::{Handle, Runtime},
};

use tokio_io::IoStream;

use librespot::core::authentication::Credentials;
use librespot::core::cache::Cache;
use librespot::core::config::{ConnectConfig, SessionConfig};
use librespot::core::session::Session;

use librespot::connect::discovery::{discovery, DiscoveryStream};
use librespot::connect::spirc::{Spirc, SpircTask};
use librespot::playback::audio_backend::{Sink, BACKENDS};
use librespot::playback::config::PlayerConfig;
use librespot::playback::mixer::{Mixer, MixerConfig};
use librespot::playback::player::{Player, PlayerEvent};

mod version;

mod meta_pipe;
use meta_pipe::{MetaPipe, MetaPipeConfig};
mod config_parser;
use config_parser::{Config, Setup};

fn usage(program: &str, opts: &getopts::Options) -> String {
    let brief = format!("Usage: {} [options]", program);
    opts.usage(&brief)
}

fn setup_logging(verbose: bool) {
    let mut builder = Builder::new();
    builder.format(|buf, record| {
        let mut base_style = buf.style();
        let mut module_style = buf.style();
        let mut level_style = buf.style();
        let mut module_path = "";
        let level = record.level();

        match level {
            log::Level::Trace | log::Level::Debug => {
                module_path = record.module_path().unwrap_or("vollibrespot");
                module_style.set_color(fmt::Color::Yellow).set_bold(true);
                level_style.set_color(fmt::Color::Green)
            }
            log::Level::Info => level_style.set_color(fmt::Color::White),
            log::Level::Warn => level_style.set_color(fmt::Color::Yellow),
            log::Level::Error => level_style.set_color(fmt::Color::Red),
        };
        level_style.set_bold(true);
        base_style.set_color(fmt::Color::Cyan).set_bold(true);
        writeln!(
            buf,
            "{} {}: {}",
            base_style.value("[Vollibrespot]"),
            module_style.value(module_path),
            level_style.value(record.args())
        )
    });
    match env::var("RUST_LOG") {
        Ok(config) => {
            builder.parse_filters(&config);
            // env::set_var("RUST_LOG",&config);
            if verbose {
                warn!("`--verbose` flag overridden by `RUST_LOG` environment variable");
            }
            builder.init();
        }
        Err(_) => {
            if verbose {
                builder.parse_filters("libmdns=info,librespot=debug,vollibrespot=trace");
            } else {
                builder.parse_filters("libmdns=info,librespot=info,vollibrespot=info");
            }
            builder.init();
        }
    }
}

fn list_backends() {
    println!("Available Backends : ");
    for (&(name, _), idx) in BACKENDS.iter().zip(0..) {
        if idx == 0 {
            println!("- {} (default)", name);
        } else {
            println!("- {}", name);
        }
    }
}

fn setup(args: &[String]) -> Setup {
    let mut opts = getopts::Options::new();
    opts.optopt(
        "c",
        "config",
        "Path to config file to read. Defaults to 'config.toml'",
        "CACHE",
    )
    .optopt(
        // This should be called something else - list compiled options?
        "",
        "backend",
        "Audio backend to use. Use '?' to list options",
        "BACKEND",
    )
    .optflag("", "verbose", "Enable verbose output");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(f) => {
            match args.last().unwrap().as_str() {
                "-v" | "--version" => {
                    println!("{}", version::version());
                    exit(0)
                }
                _ => eprintln!("error: {:?}\n{}", f, usage(&args[0], &opts)),
            }
            exit(1);
        }
    };

    let verbose = matches.opt_present("verbose");
    setup_logging(verbose);

    let backend_name = matches.opt_str("backend");
    if backend_name == Some("?".into()) {
        list_backends();
        exit(0);
    }

    println!("{}", version::version());

    let config_file = matches.opt_str("config").unwrap_or(String::from("config.toml"));
    Setup::from_config(Config::new(&config_file))
}

struct Main {
    cache: Option<Cache>,
    player_config: PlayerConfig,
    session_config: SessionConfig,
    connect_config: ConnectConfig,
    meta_config: MetaPipeConfig,
    backend: fn(Option<String>) -> Box<dyn Sink>,
    device: Option<String>,
    mixer: fn(Option<MixerConfig>) -> Box<dyn Mixer>,
    mixer_config: MixerConfig,
    handle: Handle,

    discovery: Option<DiscoveryStream>,
    signal: IoStream<()>,

    spirc: Option<Arc<Spirc>>,
    spirc_task: Option<SpircTask>,
    connect: Box<dyn Future<Item = Session, Error = io::Error>>,

    shutdown: bool,
    last_credentials: Option<Credentials>,
    auto_connect_times: Vec<Instant>,

    player_event_channel: Option<UnboundedReceiver<PlayerEvent>>,

    session: Option<Session>,
    meta_pipe: Option<MetaPipe>,
    reconnecting: bool,
}

impl Main {
    fn new(handle: Handle, setup: Setup) -> Main {
        let mut task = Main {
            handle: handle.clone(),
            cache: setup.cache,
            session_config: setup.session_config,
            player_config: setup.player_config,
            connect_config: setup.connect_config,
            meta_config: setup.meta_config,
            backend: setup.backend,
            device: setup.device,
            mixer: setup.mixer,
            mixer_config: setup.mixer_config,

            connect: Box::new(futures::future::empty()),
            discovery: None,
            spirc: None,
            spirc_task: None,
            shutdown: false,
            last_credentials: None,
            auto_connect_times: Vec::new(),
            signal: Box::new(tokio_signal::ctrl_c().flatten_stream()),

            player_event_channel: None,

            session: None,
            meta_pipe: None,
            reconnecting: false,
        };

        if setup.enable_discovery {
            let config = task.connect_config.clone();
            let device_id = task.session_config.device_id.clone();

            task.discovery = Some(
                discovery(&handle, config, device_id, setup.zeroconf_port).expect("Discovery error!"),
            );
        }

        if let Some(credentials) = setup.credentials {
            task.credentials(credentials);
        }

        task
    }

    fn credentials(&mut self, credentials: Credentials) {
        let config = self.session_config.clone();
        let handle = self.handle.clone();

        let connection = Session::connect(config, credentials, self.cache.clone(), handle);

        self.connect = connection;
        self.spirc = None;
        let task = mem::replace(&mut self.spirc_task, None);
        if let Some(task) = task {
            current_thread::spawn(Box::new(task));
            // tokio::spawn(Box::new(task));
        }
    }
}

impl Future for Main {
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        loop {
            let mut progress = false;

            if let Some(Async::Ready(Some(creds))) = self.discovery.as_mut().map(|d| d.poll().unwrap()) {
                if let Some(ref spirc) = self.spirc {
                    spirc.shutdown();
                    self.reconnecting = true;
                }
                self.auto_connect_times.clear();
                self.credentials(creds);

                progress = true;
            }

            match self.connect.poll() {
                Ok(Async::Ready(session)) => {
                    self.connect = Box::new(futures::future::empty());
                    let device = self.device.clone();
                    let mixer_config = self.mixer_config.clone();
                    let mixer = (self.mixer)(Some(mixer_config));
                    let player_config = self.player_config.clone();
                    let connect_config = self.connect_config.clone();

                    // For event hooks
                    let (event_sender, event_receiver) = channel();

                    let audio_filter = mixer.get_audio_filter();
                    let backend = self.backend;
                    let (player, event_channel) = Player::new(
                        player_config,
                        session.clone(),
                        event_sender.clone(),
                        audio_filter,
                        move || (backend)(device),
                    );

                    let (spirc, spirc_task) =
                        Spirc::new(connect_config, session.clone(), player, mixer, event_sender);

                    let spirc_ = Arc::new(spirc);

                    // Todo: improve this
                    // if !self.reconnecting {
                    let meta_config = self.meta_config.clone();
                    let meta_pipe =
                        MetaPipe::new(meta_config, session.clone(), event_receiver, spirc_.clone());
                    self.meta_pipe = Some(meta_pipe);
                    // } else {
                    //     self.meta_pipe
                    //         .reconnect(session.clone(), event_receiver, spirc_.clone());
                    // }

                    self.spirc = Some(spirc_);
                    self.spirc_task = Some(spirc_task);
                    self.player_event_channel = Some(event_channel);
                    self.session = Some(session);

                    progress = true;
                }
                Ok(Async::NotReady) => (),
                Err(error) => {
                    error!("Could not connect to server: {}", error);
                    self.connect = Box::new(futures::future::empty());
                }
            }

            if let Async::Ready(Some(())) = self.signal.poll().unwrap() {
                info!("Ctrl-C received");
                if !self.shutdown {
                    if let Some(ref spirc) = self.spirc {
                        spirc.shutdown();
                    }

                    self.shutdown = true;
                } else {
                    return Ok(Async::Ready(()));
                }

                progress = true;
            }

            let mut drop_spirc_and_try_to_reconnect = false;
            if let Some(ref mut spirc_task) = self.spirc_task {
                if let Async::Ready(()) = spirc_task.poll().unwrap() {
                    if self.shutdown {
                        return Ok(Async::Ready(()));
                    } else {
                        warn!("Spirc shut down unexpectedly");
                        drop_spirc_and_try_to_reconnect = true;
                        self.reconnecting = true;
                    }
                    progress = true;
                }
            }

            if drop_spirc_and_try_to_reconnect {
                self.spirc_task = None;
                while (!self.auto_connect_times.is_empty())
                    && ((Instant::now() - self.auto_connect_times[0]).as_secs() > 600)
                {
                    let _ = self.auto_connect_times.remove(0);
                }

                if let Some(credentials) = self.last_credentials.clone() {
                    if self.auto_connect_times.len() >= 5 {
                        warn!("Spirc shut down too often. Not reconnecting automatically.");
                    } else {
                        self.auto_connect_times.push(Instant::now());
                        self.credentials(credentials);
                    }
                }
            }

            if let Some(ref mut player_event_channel) = self.player_event_channel {
                if let Async::Ready(Some(event)) = player_event_channel.poll().unwrap() {
                    debug!("PlayerEvent:: {:?}", event);
                }
            }

            if !progress {
                return Ok(Async::NotReady);
            }
        }
    }
}

fn main() {
    if env::var("RUST_BACKTRACE").is_err() {
        env::set_var("RUST_BACKTRACE", "full")
    }
    let args: Vec<String> = std::env::args().collect();
    let mut runtime = Runtime::new().unwrap();

    // Single thread
    let handle = runtime.handle();
    //  Multithread
    // let handle = Handle::default();

    runtime.block_on(Main::new(handle, setup(&args))).unwrap();
    runtime.run().unwrap();
}
