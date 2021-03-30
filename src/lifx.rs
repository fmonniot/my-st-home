use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::UdpSocket, sync::RwLock, task::JoinHandle};

use lifx_core::{BuildOptions, Message, PowerLevel, RawMessage, HSBK};

// API with the underlying lifx machinery
pub struct LifxHandle {}

// Represent the running Lifx process
pub struct LifxTask {
    /// The task reading datagram from the network
    net_join: JoinHandle<()>,
    /// The task sending datagram to refresh the known state
    refresh_join: JoinHandle<()>,
    /// A way to send bytes to a given address
    // I use the concrete type to not have to deal with a dynamic Sink and lifetimes.
    socket: Arc<UdpSocket>,
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
    // TODO Or if we don't, rename to shutdown() and actually cancel the handles before joining.
    /// Wait for the internal tasks to finish.
    ///
    /// Note that this method consume this task, so shutting down the task before
    /// calling the join is required to gracefully stop it.
    pub async fn join_handle(self) -> Result<(), tokio::task::JoinError> {
        self.net_join.await?;
        self.refresh_join.await?;
        Ok(())
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
        let bytes: Vec<u8> = raw_msg.pack().expect("can encode lifx message").into();

        // TODO Discover this dynamically or through config
        let target: SocketAddr = "192.168.1.255:56700"
            .parse()
            .expect("correct hardcoded broadcast address");

        self.socket
            .send_to(&bytes, target)
            .await
            .expect("no error sending datagram");
    }
}

pub async fn spawn() -> Result<LifxTask, Box<dyn std::error::Error>> {
    let socket = UdpSocket::bind("0.0.0.0:56700").await?;
    socket.set_broadcast(true)?;

    let socket = Arc::new(socket);

    let source = 0x72757374;

    let bulbs = Arc::new(RwLock::new(HashMap::new()));

    let net_socket = socket.clone();
    let net_bulbs = bulbs.clone();
    let net_join = tokio::spawn(network_receive(net_socket, source, net_bulbs));

    let refresh_socket = socket.clone();
    let refresh_bulbs = bulbs.clone();
    let refresh_join = tokio::spawn(refresh_loop(refresh_bulbs, refresh_socket));

    // TODO Refresh task with config (eg. refresh config every 20 minutes)
    // Bulbs don't broadcast their state changes. The LIFX app does a state refresh every 5 seconds.

    let mut task = LifxTask {
        net_join,
        refresh_join,
        socket,
        source,
        bulbs,
    };

    task.discover().await;

    Ok(task)
}

async fn network_receive(
    socket_stream: Arc<UdpSocket>,
    source: u32,
    bulbs: Arc<RwLock<HashMap<u64, BulbInfo>>>,
) {
    let mut buf = [0; 1024];
    loop {
        let r = socket_stream.recv_from(&mut buf).await;

        match r {
            Err(error) => error!("Error while receving UDP datagram: {:?}", error),
            Ok((nbytes, addr)) => match RawMessage::unpack(&buf[0..nbytes]) {
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

                    if let Err(e) = handle_bulb_message(raw, bulb) {
                        error!("Error handling message from {}: {}", addr, e)
                    }
                }
                Err(error) => {
                    // TODO Handle
                    warn!("Error unpacking raw message from {}: {}", addr, error)
                }
            },
        }
    }
}
fn handle_bulb_message(raw: RawMessage, bulb: &mut BulbInfo) -> Result<(), lifx_core::Error> {
    let msg = Message::from_raw(&raw)?;

    debug!(
        "Handling message from bulb. message={:?}; bulb={:?}",
        msg, bulb
    );
    match msg {
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

async fn refresh_loop(bulbs: Arc<RwLock<HashMap<u64, BulbInfo>>>, socket: Arc<UdpSocket>) {
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        debug!("Refreshing known bulb information");

        // We build all our messages in one go so to release the lock before the first responses arrive
        let messages = {
            let b = bulbs.read().await;

            b.values()
                .flat_map(|bulb| {
                    let mk_message = |msg| {
                        let options = BuildOptions {
                            target: Some(bulb.target),
                            res_required: true,
                            source: bulb.source,
                            ..Default::default()
                        };

                        RawMessage::build(&options, msg)
                            .expect("Building a message should not fail")
                    };

                    vec![
                        mk_message(Message::GetLabel),
                        mk_message(Message::GetLocation),
                        mk_message(Message::GetPower),
                        mk_message(Message::LightGet),
                    ]
                    .into_iter()
                    .map(move |m| (m, bulb.addr))
                })
                .collect::<Vec<_>>()
        };

        for (message, addr) in messages {
            match socket
                .send_to(&message.pack().expect("message can be packed"), addr)
                .await
            {
                Ok(_) => (),
                Err(error) => error!(
                    "Couldn't send message to lifx bulb. message={:?}; addr={}; error={:?}",
                    message, addr, error
                ),
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
