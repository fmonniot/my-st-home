use bytes::BytesMut;
use log::{debug, error, info, trace, warn};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::{net::SocketAddr, sync::Arc};
use tokio::{net::UdpSocket, sync::RwLock, task::JoinHandle};
use tokio_util::codec::{Decoder, Encoder};

use lifx_core::{get_product_info, BuildOptions, Message, PowerLevel, RawMessage, HSBK};

// API with the underlying lifx machinery
pub struct LifxHandle {
    socket: Arc<UdpSocket>,
    bulbs: Arc<RwLock<HashMap<u64, BulbInfo>>>,
}

impl LifxHandle {
    /*

        LIFX level:
        1% in the app gave:
        HSBK { hue: 7461, saturation: 0, brightness: 1966, kelvin: 3500 }

        100% in the app gave:
        HSBK { hue: 7461, saturation: 0, brightness: 65535, kelvin: 3500 }

        So HSBK.brightness is the only interesting part, and it goes from 0 to 65535 (16 bytes)
    */
    pub async fn set_group_brightness<S: Into<String>>(&self, group: S, brightness: u16) {
        let group = group.into();
        let colors = self.colors_for_group(&group).await;
        trace!("set_group_brightness. group={}, colors={:?}", group, colors);

        for (target, source, mut color, power_level, addr) in colors {
            let options = BuildOptions {
                target: Some(target),
                res_required: true,
                source: source,
                ..Default::default()
            };

            color.brightness = brightness;

            let raw = RawMessage::build(
                &options,
                Message::LightSetColor {
                    reserved: 0,
                    color,
                    duration: 5, // ms ?
                },
            )
            .expect("Building a message should not fail");

            debug!(
                "Sending brightness change. brightness={:?}, addr={:?}",
                brightness, addr
            );
            self.send_raw(raw, addr).await;

            // Brightness needs to be at least 1000 otherwise we turn on/off the bulb
            let turn = power_level.and_then(|p| match p {
                PowerLevel::Enabled if brightness <= 1000 => Some(PowerLevel::Standby),
                PowerLevel::Standby if brightness >= 1000 => Some(PowerLevel::Enabled),
                _ => None,
            });

            if let Some(level) = turn {
                let raw = RawMessage::build(&options, Message::SetPower { level })
                    .expect("Building a message should not fail");

                debug!(
                    "Sending power level change. level={:?}, addr={:?}",
                    level, addr
                );
                self.send_raw(raw, addr).await;
            }
        }
    }

    async fn colors_for_group(
        &self,
        group: &String,
    ) -> Vec<(u64, u32, HSBK, Option<PowerLevel>, SocketAddr)> {
        let bulbs = self.bulbs.read().await;

        bulbs
            .values()
            .filter(|b| match &b.group {
                Some(b) => b == group,
                None => false,
            })
            .filter_map(|bulb| match bulb.color {
                Color::Single(Some(hsbk)) => {
                    Some((bulb.target, bulb.source, hsbk, bulb.power_level, bulb.addr))
                }
                _ => None,
            })
            .collect()
    }

    async fn send_raw(&self, raw: RawMessage, addr: SocketAddr) {
        match self
            .socket
            .send_to(&raw.pack().expect("message can be packed"), addr)
            .await
        {
            Ok(_) => (),
            Err(error) => error!(
                "Couldn't send message to lifx bulb. message={:?}; addr={}; error={:?}",
                raw, addr, error
            ),
        }
    }
}

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
        let socket = self.socket.clone();
        let bulbs = self.bulbs.clone();

        LifxHandle { socket, bulbs }
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
                        error!("Error handling message from {}: {:?}", addr, e)
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

    trace!(
        "Handling message from bulb. message={:?}; bulb={:?}",
        msg,
        bulb
    );
    match msg {
        Message::StateService { port, service } => {
            if port != bulb.addr.port() as u32 {
                warn!("Unsupported service: {:?}/{}", service, port);
            }
        }
        Message::StateVersion {
            vendor, product, ..
        } => {
            //bulb.model.update((vendor, product));
            if let Some(info) = get_product_info(vendor, product) {
                if info.multizone {
                    bulb.color = Color::Multi(None)
                } else {
                    bulb.color = Color::Single(None)
                }
            }
        }
        Message::StateLabel { label } => bulb.name = Some(label.0),
        Message::StateLocation { label, .. } => bulb.location = Some(label.0),
        Message::StateGroup { label, .. } => bulb.group = Some(label.0),
        Message::LightState {
            color,
            power,
            label,
            ..
        } => {
            // TODO What to do on unknown
            if let Color::Single(ref mut d) = bulb.color {
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

                    // TODO Maybe put that into a bulb method and generate them based on what we know
                    // thinking about color here.
                    vec![
                        mk_message(Message::GetLabel),
                        mk_message(Message::GetLocation),
                        mk_message(Message::GetGroup),
                        mk_message(Message::GetVersion),
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
    group: Option<String>,
    power_level: Option<PowerLevel>,
    color: Color,
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
            group: None,
            power_level: None,
            color: Color::Unknown,
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
    Single(Option<HSBK>),             // Regular bulb
    Multi(Option<Vec<Option<HSBK>>>), // strip, beam and candles
}

#[derive(Debug, Clone)]
struct LifxCodec;

#[derive(Debug, Clone)]
enum LifxError {
    /// This error means we were unable to parse a raw message because its type is unknown.
    ///
    /// LIFX devices are known to send messages that are not officially documented, so this error
    /// type does not necessarily represent a bug.
    UnknownMessageType(u16),

    /// This error means one of the message fields contains an invalid or unsupported value.
    ///
    /// The inner string is a description of the error.
    ProtocolError(String),
    // serialize the underlying error to be able to make LifxError Clone
    // TODO See if we can remove Clone from the Message trait, and only asks
    // for it on demand (like in channel).
    Io(String),
}

impl crate::actor::Message for LifxError {}

impl std::convert::From<lifx_core::Error> for LifxError {
    fn from(e: lifx_core::Error) -> Self {
        match e {
            lifx_core::Error::UnknownMessageType(u) => LifxError::UnknownMessageType(u),
            lifx_core::Error::ProtocolError(s) => LifxError::ProtocolError(s),
            lifx_core::Error::Io(e) => e.into(),
        }
    }
}

impl std::convert::From<std::io::Error> for LifxError {
    fn from(e: std::io::Error) -> Self {
        LifxError::Io(format!("{}", e))
    }
}

impl Encoder<(BuildOptions, Message)> for LifxCodec {
    type Error = LifxError;

    fn encode(
        &mut self,
        (options, msg): (BuildOptions, Message),
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        let raw = RawMessage::build(&options, msg)?;

        let bytes = raw.pack()?;

        dst.extend_from_slice(&bytes);

        Ok(())
    }
}

impl Decoder for LifxCodec {
    type Item = Message;
    type Error = LifxError;

    // TODO Manage frames over multiple packet (is it even possible with lify protocol ?)
    // TODO Make sure we have correctly consumed the buffer
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let raw = RawMessage::unpack(&src)?;
        let msg = Message::from_raw(&raw)?;

        Ok(Some(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_and_decode() {
        let options = BuildOptions {
            target: Some(1),
            res_required: true,
            source: 2,
            ..Default::default()
        };

        let orig_color = HSBK {
            hue: 5,
            saturation: 6,
            brightness: 7,
            kelvin: 8,
        };
        let message = Message::LightSetColor {
            reserved: 0,
            color: orig_color.clone(),
            duration: 9, // ms ?
        };

        let mut buffer = BytesMut::new();
        LifxCodec
            .encode((options, message.clone()), &mut buffer)
            .unwrap();
        let res = LifxCodec.decode(&mut buffer);

        match res {
            Ok(Some(Message::LightSetColor {
                reserved,
                color,
                duration,
            })) => {
                assert_eq!(reserved, 0);
                assert_eq!(color, orig_color);
                assert_eq!(duration, 9);
            }
            r => assert!(false, "result {:?} was not expected", r),
        }
    }
}

/// An alternative to the current implementation using actors
/// instead of raw tokio's primitive.
pub mod actors {

    use super::{LifxCodec, LifxError, Color};
    use crate::actor::{Actor, ActorRef, Context, Message, Receiver, Timer, network::{self, udp::Udp}};
    use log::error;
    use std::time::{Duration, Instant};
    use std::collections::HashMap;
    use lifx_core::{get_product_info, BuildOptions, PowerLevel, RawMessage, HSBK};

    // Actors

    #[derive(Default)]
    pub struct Manager {
        udp: Option<Udp<LifxCodec>>,
        bulbs: HashMap<u64, BulbInfo>,
    }

    struct BulbInfo {
        last_seen: Instant,
        source: u32,
        target: u64,
        addr: ActorRef<Bulb>,
        // Option because we need some network interaction before having this information
        name: Option<String>,
        location: Option<String>,
        group: Option<String>,
        power_level: Option<PowerLevel>,
        color: Color,
    }

    struct Bulb;

    impl Actor for Bulb {

    }

    // Messages

    #[derive(Debug, Clone)]
    pub enum Management {
        Discover,
        Refresh,
    }
    impl Message for Management {}

    #[derive(Debug, Clone)]
    pub enum Command {
        SetGroupBrightness { group: String, brightness: u16 },
        GetGroupColors { group: String },
    }
    impl Message for Command {}

    // Implementations

    impl Actor for Manager {
        fn pre_start(&mut self, ctx: &Context<Self>) {
            // Create the UDP actor
            let addr = "0.0.0.0:56700"
                .parse()
                .expect("correct hardcoded broadcast address");

            match network::udp::create(addr, LifxCodec, ctx.myself.clone()) {
                Ok(udp) => self.udp = Some(udp),
                Err(err) => error!(
                    "Can't create the UDP socket for LIFX communication: {:?}",
                    err
                ),
            };

            // Set up refresh trigger
            let delay = Duration::from_secs(9);
            let _ = ctx.schedule(delay, delay, ctx.myself.clone(), Management::Refresh);
        }

        fn post_stop(&mut self, _ctx: &Context<Self>) {}
    }

    impl Manager {
        fn discovery(&self) {}

        fn refresh(&self) {}

        fn set_group_brightness(&self, _group: String, _brightness: u16) {}

        fn get_group_colors(&self, _group: String) {}
    }

    // Receiver

    impl Receiver<Management> for Manager {
        fn recv(&mut self, ctx: &Context<Self>, msg: Management) {
            match msg {
                Management::Discover => self.discovery(),
                Management::Refresh => self.refresh(),
            }
        }
    }

    impl Receiver<Command> for Manager {
        fn recv(&mut self, ctx: &Context<Self>, msg: Command) {
            match msg {
                Command::SetGroupBrightness { group, brightness } => {
                    self.set_group_brightness(group, brightness)
                }
                Command::GetGroupColors { group } => self.get_group_colors(group),
            }
        }
    }

    impl Receiver<UdpMessage> for Manager {
        fn recv(&mut self, _ctx: &Context<Self>, msg: UdpMessage) {
            match msg {
                Ok((_message, _addr)) => {
                    // TODO Find or create the bulb actor
                }
                Err(error) => {
                    error!("Error handling message: {:?}", error)
                }
            }
        }
    }

    // Help the compiler inferring the Message trait for some types
    type UdpSend = (lifx_core::BuildOptions, lifx_core::Message);
    impl Message for UdpSend {}
    impl Message for lifx_core::Message {}

    type UdpMessage = std::result::Result<(lifx_core::Message, std::net::SocketAddr), LifxError>;
}
