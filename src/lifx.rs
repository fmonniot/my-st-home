use tokio::{net::UdpSocket, task::JoinHandle};
use tokio_util::udp::UdpFramed;
use tokio_util::codec::BytesCodec;
use futures::{SinkExt, Stream, StreamExt, stream::SplitSink};
use std::time::{Duration, Instant};
use std::net::SocketAddr;
use bytes::{Bytes, BytesMut};

use lifx_core::{get_product_info, BuildOptions, Message, PowerLevel, RawMessage, Service, HSBK};

// API with the underlying lifx machinery
pub struct LifxHandle {

}

// Represent the running Lifx process
pub struct LifxTask {
    // The task reading datagram from the network
    net_join: JoinHandle<()>,
    /// A way to send bytes to a given address
    // I use the concrete type to not have to deal with a dynamic Sink and lifetimes.
    socket_sink: SplitSink<UdpFramed<BytesCodec>, (Bytes, SocketAddr)>,
    /// An identifier for this process
    source: u32,
}

impl LifxTask {
    pub fn handle(&self) -> LifxHandle {
        LifxHandle {}
    }

    // TODO Return a Result (instead of expecting everything)
    pub async fn discover(&mut self) {
        println!("Starting bulbs discovery");

        let opts = BuildOptions {
            source: self.source,
            ..Default::default()
        };
        let raw_msg = RawMessage::build(&opts, Message::GetService).expect("GetService raw message");
        let bytes = raw_msg.pack().expect("can encode lifx message").into();

        let target = "192.168.1.255:56700".parse().expect("correct hardcoded broadcast address");

        self.socket_sink.send((bytes, target)).await.expect("no error sending datagram");
    }
}

pub async fn spawn() -> Result<LifxTask, Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:56700").await?;
    socket.set_broadcast(true)?;

    let (mut socket_sink, mut socket_stream) =
        UdpFramed::new(socket, BytesCodec::new()).split();

    let source = 0x72757374;
    
    let net_join = tokio::spawn(network_receive(socket_stream, source));

    let mut task = LifxTask {net_join, socket_sink, source};

    task.discover().await;

    Ok(task)
}

async fn network_receive<S: Stream<Item = Result<(BytesMut, SocketAddr), std::io::Error>> + Unpin>(mut socket_stream: S, source: u32) {
    while let Some(res) = socket_stream.next().await {
        match res {
            Ok((bytes, addr)) => {
                match RawMessage::unpack(&bytes) {
                    Ok(raw) => {
                        if raw.frame_addr.target == 0 {
                            println!("raw.frame_addr.target == 0 for raw={:?}", raw);
                            continue;
                        }

                        let bulb = BulbInfo::new(source, raw.frame_addr.target, addr);
                        println!("Received messages from bulb {:?}", bulb);
                    }
                    Err(error) => {
                        // TODO Handle
                        println!("Error unpacking raw message from {}: {}", addr, error)
                    }
                }
            }
            Err(error) => {
                // TODO Handle correctly
                println!("Error while reading udp datagram for lifx")
            }
        }
    }
}

#[derive(Debug)]
struct BulbInfo {
    last_seen: Instant,
    source: u32,
    target: u64,
    addr: SocketAddr,
    //name: String,
    //model: (u32, u32),
    //location: String,
    //host_firmware: u32,
    //wifi_firmware: u32,
    //power_level: PowerLevel,
    //color: Color,
}

impl BulbInfo {
    fn new(source: u32, target: u64, addr: SocketAddr) -> BulbInfo {
        BulbInfo {
            last_seen: Instant::now(),
            source,
            target,
            addr
        }
    }

    fn update(&mut self, addr: SocketAddr) {
        self.last_seen = Instant::now();
        self.addr = addr;
    }
}