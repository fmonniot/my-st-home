use crate::actor::{
    network::{self, udp, udp::Udp},
    Actor, ActorRef, Context, Message, Receiver, Timer,
};
use bytes::BytesMut;
use lifx_core::{get_product_info, BuildOptions, FrameAddress, PowerLevel, RawMessage, HSBK};
use log::{debug, error, trace, warn};
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant}, convert::TryInto,
};
use tokio_util::codec::{Decoder, Encoder};

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

impl Encoder<(BuildOptions, lifx_core::Message)> for LifxCodec {
    type Error = LifxError;

    fn encode(
        &mut self,
        (options, msg): (BuildOptions, lifx_core::Message),
        dst: &mut BytesMut,
    ) -> Result<(), Self::Error> {
        let raw = RawMessage::build(&options, msg)?;

        let bytes = raw.pack()?;

        dst.extend_from_slice(&bytes);

        Ok(())
    }
}

impl Decoder for LifxCodec {
    type Item = (FrameAddress, lifx_core::Message);
    type Error = LifxError;

    // TODO Manage frames over multiple packet (is it even possible with lify protocol ?)
    // TODO Make sure we have correctly consumed the buffer
    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let raw = match RawMessage::unpack(&src) {
            Ok(r) => r,
            Err(lifx_core::Error::Io(io)) if io.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(None)
            }
            Err(error) => {
                debug!("Error unpacking raw message: {:?}", error);
                return Err(error.into());
            }
        };

        // We have read some bytes, so we need to remove them from the buffer.
        let size = raw.packed_size();
        let _ = src.split_to(size);

        let msg = lifx_core::Message::from_raw(&raw)?;

        Ok(Some((raw.frame_addr, msg)))
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
        let message = lifx_core::Message::LightSetColor {
            reserved: 0,
            color: orig_color.clone(),
            duration: 9, // ms ?
        };

        // Encode two messages back to back
        let mut buffer = BytesMut::new();
        LifxCodec
            .encode((options.clone(), message.clone()), &mut buffer)
            .unwrap();
        LifxCodec
            .encode((options, message.clone()), &mut buffer)
            .unwrap();

        let test = |res| match res {
            Ok(Some((
                FrameAddress { target, .. },
                lifx_core::Message::LightSetColor {
                    reserved,
                    color,
                    duration,
                },
            ))) => {
                assert_eq!(target, 1);
                assert_eq!(reserved, 0);
                assert_eq!(color, orig_color);
                assert_eq!(duration, 9);
            }
            r => assert!(false, "result {:?} was not expected", r),
        };

        // There are two messages following each other. A decoder should remove a decoded message
        // from the buffer once it has read it.
        test(LifxCodec.decode(&mut buffer));
        test(LifxCodec.decode(&mut buffer));
    }
}

// Actors

pub struct Manager {
    udp: Option<ActorRef<udp::Udp<LifxCodec>>>,
    bulbs: HashMap<u64, BulbInfo>,
    broadcast_addr: SocketAddr,
    source: u32,
}

#[derive(Debug)]
struct BulbInfo {
    last_seen: Instant,
    source: u32,
    target: u64,
    address: SocketAddr,
    // Option because we need some network interaction before having this information
    name: Option<String>,
    location: Option<String>,
    group: Option<String>,
    power_level: Option<PowerLevel>,
    color: Color,
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
}
impl Message for Command {}

// Implementations

impl Actor for Manager {
    fn pre_start(&mut self, ctx: &Context<Self>) {
        // Socket to listen on. We want to receive everything from the network
        // on the LIFX port (56700).
        let addr = "0.0.0.0:56700"
            .parse()
            .expect("correct hardcoded broadcast address");

        // Create the UDP actor
        let a: Result<ActorRef<Udp<LifxCodec>>, Box<dyn std::error::Error>> =
            network::udp::create(addr, LifxCodec, ctx.myself.clone(), true)
                .map_err(|e| e.into())
                .and_then(|actor| {
                    ctx.actor_of("lifx/broadcast_socket", actor)
                        .map_err(|e| e.into())
                });

        match a {
            Ok(udp) => self.udp = Some(udp),
            Err(err) => error!(
                "Can't create the UDP socket for LIFX communication: {:?}",
                err
            ),
        };

        // Set up refresh trigger
        let delay = Duration::from_secs(9);
        let _ = ctx.schedule(delay, delay, ctx.myself.clone(), Management::Refresh);

        // trigger discovery
        ctx.myself.send_msg(Management::Discover);
    }

    fn post_stop(&mut self, _ctx: &Context<Self>) {}
}

impl Manager {
    pub fn new(broadcast_addr: SocketAddr) -> Manager {
        let source = 0x72757374;

        Manager {
            udp: None,
            bulbs: HashMap::new(),
            source,
            broadcast_addr,
        }
    }

    fn discovery(&self) {
        let opts = BuildOptions {
            source: self.source,
            ..Default::default()
        };
        let message = lifx_core::Message::GetService;

        if let Some(udp) = &self.udp {
            udp.send_msg(udp::Msg::WriteTo((opts, message), self.broadcast_addr))
        }
    }

    fn refresh(&self) {
        debug!("Refreshing LIFX bulbs");
        // First make a list of all the messages we want to send
        let messages = vec![
            lifx_core::Message::GetLabel,
            lifx_core::Message::GetLocation,
            lifx_core::Message::GetGroup,
            lifx_core::Message::GetVersion,
            lifx_core::Message::GetPower,
            lifx_core::Message::LightGet,
        ];

        // Then iterate on each bulb and send them said messages
        for (_, bulb) in &self.bulbs {
            for message in &messages {
                if let Some(udp) = &self.udp {
                    let m = (bulb.build_options(), message.clone());
                    udp.send_msg(udp::Msg::WriteTo(m, bulb.address))
                }
            }
        }
    }

    /*
        LIFX level:
        1% in the app gave:
        HSBK { hue: 7461, saturation: 0, brightness: 1966, kelvin: 3500 }

        100% in the app gave:
        HSBK { hue: 7461, saturation: 0, brightness: 65535, kelvin: 3500 }

        So HSBK.brightness is the only interesting part, and it goes from 0 to 65535 (16 bytes)
    */
    fn set_group_brightness(&self, group: String, brightness: u16) {
        let colors = self.get_group_colors(&group);
        trace!("set_group_brightness. group={}, colors={:?}", group, colors);

        for (target, source, mut color, power_level, addr) in colors {
            let options = BuildOptions {
                target: Some(target),
                res_required: true,
                source: source,
                ..Default::default()
            };

            color.brightness = brightness;

            let message = lifx_core::Message::LightSetColor {
                reserved: 0,
                color,
                duration: 5, // ms ?
            };

            debug!(
                "Sending brightness change. brightness={:?}, addr={:?}",
                brightness, addr
            );
            if let Some(socket) = &self.udp {
                socket.send_msg(udp::Msg::WriteTo((options.clone(), message), addr));
            }

            // Brightness needs to be at least 1000 otherwise we turn on/off the bulb
            let turn = power_level.and_then(|p| match p {
                PowerLevel::Enabled if brightness <= 1000 => Some(PowerLevel::Standby),
                PowerLevel::Standby if brightness >= 1000 => Some(PowerLevel::Enabled),
                _ => None,
            });

            if let Some(level) = turn {
                let message = lifx_core::Message::SetPower { level };

                debug!(
                    "Sending power level change. level={:?}, addr={:?}",
                    level, addr
                );
                if let Some(socket) = &self.udp {
                    socket.send_msg(udp::Msg::WriteTo((options, message), addr));
                }
            }
        }
    }

    fn get_group_colors(
        &self,
        group: &String,
    ) -> Vec<(u64, u32, HSBK, Option<PowerLevel>, SocketAddr)> {
        self.bulbs
            .values()
            .filter(|b| match &b.group {
                Some(b) => b == group,
                None => false,
            })
            .filter_map(|bulb| match bulb.color {
                Color::Single(Some(hsbk)) => Some((
                    bulb.target,
                    bulb.source,
                    hsbk,
                    bulb.power_level,
                    bulb.address,
                )),
                _ => None,
            })
            .collect()
    }
}

impl BulbInfo {
    fn new(source: u32, target: u64, address: SocketAddr) -> BulbInfo {
        BulbInfo {
            last_seen: Instant::now(),
            source,
            target,
            address,
            name: None,
            location: None,
            group: None,
            power_level: None,
            color: Color::Unknown,
        }
    }

    fn build_options(&self) -> BuildOptions {
        BuildOptions {
            target: Some(self.target),
            res_required: true,
            source: self.source,
            ..Default::default()
        }
    }

    fn update(&mut self, address: SocketAddr) {
        self.last_seen = Instant::now();
        self.address = address;
    }
}

// Receiver

impl Receiver<Management> for Manager {
    fn recv(&mut self, _ctx: &Context<Self>, msg: Management) {
        match msg {
            Management::Discover => self.discovery(),
            Management::Refresh => self.refresh(),
        }
    }
}

impl Receiver<Command> for Manager {
    fn recv(&mut self, _ctx: &Context<Self>, msg: Command) {
        match msg {
            Command::SetGroupBrightness { group, brightness } => {
                self.set_group_brightness(group, brightness)
            }
        }
    }
}

impl Receiver<UdpMessage> for Manager {
    fn recv(&mut self, _ctx: &Context<Self>, msg: UdpMessage) {
        match msg {
            Ok(((frame_addr, message), addr)) => {
                // A target of zero means that message target all devices
                // (broadcast style). We aren't a device, so we skip over
                // those messages.
                if frame_addr.target == 0 {
                    debug!("frame_addr.target == 0 for message={:?}", message);
                    return;
                }

                let source = self.source;
                let bulb = self
                    .bulbs
                    .entry(frame_addr.target)
                    .and_modify(|bulb| bulb.update(addr))
                    .or_insert_with(|| BulbInfo::new(source, frame_addr.target, addr));

                handle_bulb_message(bulb, message);
            }
            Err(error) => {
                error!("Error handling message: {:?}", error)
            }
        }
    }
}

fn handle_bulb_message(bulb: &mut BulbInfo, message: lifx_core::Message) {
    trace!(
        "Handling message from bulb. message={:?}; bulb={:?}",
        message,
        bulb
    );
    match message {
        lifx_core::Message::StateService { port, service } => {
            if port != bulb.address.port() as u32 {
                warn!("Unsupported service: {:?}/{}", service, port);
            }
        }
        lifx_core::Message::StateVersion {
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
        lifx_core::Message::StateLabel { label } => bulb.name = Some(label.to_string()),
        lifx_core::Message::StateLocation { label, .. } => bulb.location = Some(label.to_string()),
        lifx_core::Message::StateGroup { label, .. } => bulb.group = Some(label.to_string()),
        lifx_core::Message::LightState {
            color,
            power,
            label,
            ..
        } => {
            // TODO What to do on unknown
            if let Color::Single(ref mut d) = bulb.color {
                d.replace(color);

                bulb.power_level = power.try_into().ok();
            }
            bulb.name = Some(label.to_string());
        }
        lifx_core::Message::StatePower { level } => bulb.power_level = level.try_into().ok(),
        unsupported => {
            debug!("Received unsupported message: {:?}", unsupported);
        }
    };
}

// Help the compiler inferring the Message trait for some types
type UdpSend = (lifx_core::BuildOptions, lifx_core::Message);
impl Message for UdpSend {}
impl Message for lifx_core::Message {}
impl Message for (lifx_core::FrameAddress, lifx_core::Message) {}

type UdpMessage = std::result::Result<
    (
        (lifx_core::FrameAddress, lifx_core::Message),
        std::net::SocketAddr,
    ),
    LifxError,
>;
