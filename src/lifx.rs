use bytes::{Bytes, BytesMut};
use futures::{stream::SplitSink, SinkExt, Stream, StreamExt};
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::time::Instant;
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::UdpSocket, sync::RwLock, task::JoinHandle};
use tokio_util::codec::BytesCodec;
use tokio_util::udp::UdpFramed;

use lifx_core::{BuildOptions, Message, RawMessage, HSBK, PowerLevel};

// API with the underlying lifx machinery
pub struct LifxHandle {}

// Represent the running Lifx process
pub struct LifxTask {
    // The task reading datagram from the network
    net_join: JoinHandle<()>,
    /// A way to send bytes to a given address
    // I use the concrete type to not have to deal with a dynamic Sink and lifetimes.
    socket_sink: SplitSink<UdpFramed<BytesCodec>, (Bytes, SocketAddr)>,
    /// An identifier for this process
    source: u32,
    /// Map target id to its bulb information
    bulbs: Arc<RwLock<HashMap<u64, BulbInfo>>>,
}

impl LifxTask {
    pub fn handle(&self) -> LifxHandle {
        LifxHandle {}
    }

    // TODO Might be better to implement the trait ourselves
    pub async fn join_handle(self) -> Result<(), tokio::task::JoinError> {
        self.net_join.await
    }

    // TODO Return a Result (instead of expecting everything)
    pub async fn discover(&mut self) {
        info!("Starting LIFX bulbs discovery");

        let opts = BuildOptions {
            source: self.source,
            ..Default::default()
        };
        let raw_msg =
            RawMessage::build(&opts, Message::GetService).expect("GetService raw message");
        let bytes = raw_msg.pack().expect("can encode lifx message").into();

        let target = "192.168.1.255:56700"
            .parse()
            .expect("correct hardcoded broadcast address");

        self.socket_sink
            .send((bytes, target))
            .await
            .expect("no error sending datagram");
    }
}

pub async fn spawn() -> Result<LifxTask, Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:56700").await?;
    socket.set_broadcast(true)?;

    let (socket_sink, socket_stream) = UdpFramed::new(socket, BytesCodec::new()).split();

    let source = 0x72757374;

    let bulbs = Arc::new(RwLock::new(HashMap::new()));
    let net_bulbs = bulbs.clone();

    let net_join = tokio::spawn(network_receive(socket_stream, source, net_bulbs));

    // TODO Refresh task with config (eg. refresh config every 20 minutes)
    // Bulbs don't broadcast their state changes. The LIFX app does a state refresh every 5 seconds.

    let mut task = LifxTask {
        net_join,
        socket_sink,
        source,
        bulbs,
    };

    task.discover().await;

    Ok(task)
}

async fn network_receive<
    S: Stream<Item = Result<(BytesMut, SocketAddr), std::io::Error>> + Unpin,
>(
    mut socket_stream: S,
    source: u32,
    bulbs: Arc<RwLock<HashMap<u64, BulbInfo>>>,
) {
    while let Some(res) = socket_stream.next().await {
        match res {
            Ok((bytes, addr)) => {
                match RawMessage::unpack(&bytes) {
                    Ok(raw) => {
                        if raw.frame_addr.target == 0 {
                            debug!("raw.frame_addr.target == 0 for raw={:?}", raw);
                            continue;
                        }

                        let mut bulbs = bulbs.write().await;
                        let bulb = bulbs
                            .entry(raw.frame_addr.target)
                            .and_modify(|bulb| bulb.update(addr))
                            .or_insert_with(|| BulbInfo::new(source, raw.frame_addr.target, addr));
                        debug!("Received message from bulb {:?}", bulb);

                        if let Err(e) = handle_bulb_message(raw, bulb) {
                            error!("Error handling message from {}: {}", addr, e)
                        }
                    }
                    Err(error) => {
                        // TODO Handle
                        warn!("Error unpacking raw message from {}: {}", addr, error)
                    }
                }
            }
            Err(error) => {
                // TODO Handle correctly
                warn!("Error while reading udp datagram for lifx: {}", error)
            }
        }
    }
}

fn handle_bulb_message(raw: RawMessage, bulb: &mut BulbInfo) -> Result<(), lifx_core::Error> {
    match Message::from_raw(&raw)? {
        Message::StateService { port, service } => {
            if port != bulb.addr.port() as u32 {
                warn!("Unsupported service: {:?}/{}", service, port);
            }
        }
        Message::StateLabel { label } => bulb.name = Some(label.0),
        Message::StateLocation { label, .. } => bulb.location = Some(label.0),
        Message::LightState {
            color,
            power,
            label,
            ..
        } => {
            if let Some(Color::Single(ref mut d)) = bulb.color {
                d.replace(color);

                bulb.power_level = Some(power);
            }
            bulb.name = Some(label.0);
        }
        Message::StatePower { level } => bulb.power_level = Some(level),
        unsupported => {
            debug!("Received unsupported message: {:?}", unsupported);
        }
    };

    Ok(())
}

#[derive(Debug)]
struct BulbInfo {
    last_seen: Instant,
    source: u32,
    target: u64,
    addr: SocketAddr,
    // Option because we need some network interaction before having this information
    name: Option<String>,
    location: Option<String>,
    power_level: Option<PowerLevel>,
    color: Option<Color>,
}

impl BulbInfo {
    fn new(source: u32, target: u64, addr: SocketAddr) -> BulbInfo {
        BulbInfo {
            last_seen: Instant::now(),
            source,
            target,
            addr,
            name: None,
            location: None,
            power_level: None,
            color: None,
        }
    }

    fn update(&mut self, addr: SocketAddr) {
        self.last_seen = Instant::now();
        self.addr = addr;
    }
}

#[derive(Debug)]
enum Color {
    Unknown,
    Single(Option<HSBK>),
    Multi(Option<Vec<Option<HSBK>>>),
}
