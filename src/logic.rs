//! Contains the various bit of logic which interconnect the different
//! sensors, actuactors and platform together.

use crate::{lifx::LifxHandle, mqtt::Command};
use crate::{mqtt::DeviceEvent, screen::ScreenHandle, sensors::SensorMessage};
use futures::{Sink, SinkExt, Stream, StreamExt};
use log::{debug, trace, warn};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

pub(super) async fn st_light_state<St, Si>(
    mut commands: St,
    events: Si,
    lifx: LifxHandle,
    light_state: Arc<RwLock<bool>>,
    _screen: ScreenHandle,
) where
    St: Stream<Item = Result<Command, BroadcastStreamRecvError>> + Unpin,
    Si: Sink<DeviceEvent>,
{
    let mut events = Box::pin(events);
    while let Some(message) = commands.next().await {
        match message {
            Err(error) => {
                warn!("Cannot pull command. error={:?}", error);
            }
            Ok(command) => {
                // TODO Correctly handle different type of command
                let switch = &command.command == "on";

                let current = { *light_state.read().await };

                debug!(
                    "Received command '{}', current state is '{}'",
                    switch, current
                );
                if switch != current {
                    // Change light_state
                    {
                        let mut s = light_state.write().await;
                        *s = switch;
                    }

                    let value = if switch { "on" } else { "off" };

                    // Send device event
                    debug!("Sending device event with value '{}'", value);
                    match events
                        .send(DeviceEvent::simple_str("main", "switch", "switch", value))
                        .await
                    {
                        Ok(()) => (),
                        Err(_) => {
                            warn!("Error sending device event"); // TODO Error
                        }
                    }

                    // Change lifx power level
                    // TODO Don't change the brightness setting, only power
                    lifx.set_group_brightness("Living Room - Desk", 0).await;
                }
            }
        }
    }
}

pub(super) async fn adaptive_brightness<S>(
    mut sensors: S,
    lifx: LifxHandle,
    light_state: Arc<RwLock<bool>>,
    screen: ScreenHandle,
) where
    S: Stream<Item = Result<SensorMessage, BroadcastStreamRecvError>> + Unpin,
{
    debug!("Starting sensors run loop");

    // TODO Needs to make that configurable without code change
    let mut controller = pid_lite::Controller::new(80.0, 100.0, 0.01, 0.01);
    let mut brightness_command: u16 = 1000;

    while let Some(message) = sensors.next().await {
        if !*light_state.read().await {
            debug!("light turned off, skipping sensor message");
            continue;
        }

        match message {
            Ok(SensorMessage::Luminosity { lux, .. }) => {
                let correction = controller.update(lux as f64);
                let next_bright = brightness_command as f64 + correction;
                trace!("next_bright after correction: {}", next_bright);

                let brightness = match next_bright.round() as i64 {
                    n if n < 0 => 0,
                    n if n <= std::u16::MAX as i64 => n as u16,
                    _ => std::u16::MAX,
                };

                debug!("Setting brightness to {}", brightness);
                brightness_command = brightness;
                lifx.set_group_brightness("Living Room - Desk", brightness)
                    .await;
                screen
                    .update_group_brightness("Living Room - Desk", brightness)
                    .unwrap(); // TODO Error handling
            }
            Err(error) => {
                warn!("Cannot pull sensor data. error={:?}", error);
            }
        }
    }

    debug!("Stopped sensors run loop");
}

pub mod st_state {
    use crate::{
        actor::{Actor, ActorRef, Context, Receiver},
     lifx::actors::Command as LifxCommand,
      smartthings::{Command, Cmd as SmartThingsCmd, DeviceEvent},
    };
    use super::brightness::{Command as BrightnessCommand};
    use log::{debug};

    // TODO Keep the generic or use the struct directly ?
    pub struct StState<B: Actor, L: Actor, S: Actor> {
        brightness: ActorRef<B>,
        lifx: ActorRef<L>,
        smartthings: ActorRef<S>,
        light: bool,
    }

    pub fn new<B, L, S>(brightness: ActorRef<B>, lifx: ActorRef<L>, smartthings: ActorRef<S>) -> StState<B, L, S>
    where
        B: Receiver<BrightnessCommand>,
        L: Receiver<LifxCommand>,
        S: Receiver<SmartThingsCmd>,
    {
        StState {
            brightness,
             lifx,
             smartthings,
             light: false,
        }
    }

    impl<B, L, S> Actor for StState<B, L, S>
    where
    B: Receiver<BrightnessCommand>,
    L: Receiver<LifxCommand>,
    S: Receiver<SmartThingsCmd>,
     {

        fn pre_start(&mut self, ctx: &Context<Self>) {
            let channel = ctx.channel::<Command>();
            channel.subscribe_to(ctx.myself.clone(), "smartthings/command");
        }
    }

    impl<B, L, S> Receiver<Command> for StState<B, L, S>
    where
    B: Receiver<BrightnessCommand>,
    L: Receiver<LifxCommand>,
    S: Receiver<SmartThingsCmd>,
    {
        fn recv(&mut self, _ctx: &Context<Self>, command: Command) {
            // TODO Correctly handle different type of command
            let switch = &command.command == "on";

            let current = self.light;

            debug!(
                "Received command '{}', current state is '{}'",
                switch, current
            );
            if switch != current {
                // Change light_state
                self.light = switch;

                let value = if switch { "on" } else { "off" };

                // Send device event to the ST platform
                debug!("Sending device event with value '{}'", value);

                self.smartthings.send_msg(SmartThingsCmd::Publish(DeviceEvent::simple_str("main", "switch", "switch", value)));

                // Let the brightness logic know about this
                // TODO Some thinking to do on how many level of indirection we really need
                let cmd = if switch { BrightnessCommand::TurnOn } else { BrightnessCommand::TurnOff };
                self.brightness.send_msg(cmd);

                // Change lifx power level
                // TODO Don't change the brightness setting, only power
                self.lifx.send_msg(LifxCommand::SetGroupBrightness {
                    group: "Living Room - Desk".to_string(),
                    brightness: 0
                 });
            }
        }
    }


}
pub mod brightness {
    use crate::{
        actor::{Actor, ActorRef, Context, Receiver},
        lifx::actors::Command as LifxCommand,
        sensors::actors::BroadcastedSensorRead,
    };
    use log::{debug, trace};
    use pid_lite::Controller;

    pub struct AdaptiveBrightness<AL: Receiver<LifxCommand>> {
        controller: Controller,
        brightness_command: u16,
        light_turned_on: bool,
        lifx: ActorRef<AL>,
    }

    pub fn new<A>(lifx: ActorRef<A>) -> AdaptiveBrightness<A>
    where
        A: Receiver<LifxCommand>,
    {
        AdaptiveBrightness {
            controller: Controller::new(80.0, 100.0, 0.01, 0.01),
            brightness_command: 1000,
            light_turned_on: false,
            lifx,
        }
    }

    // TODO Will be used in the ST state actor, when it comes to it
    #[derive(Debug, Clone)]
    pub enum Command {
        TurnOn,
        TurnOff,
    }

    impl crate::actor::Message for Command {}

    impl<AL: Receiver<LifxCommand>> Actor for AdaptiveBrightness<AL> {
        fn pre_start(&mut self, ctx: &Context<Self>) {
            debug!("Starting adaptive brightness actor");

            let sensor_channel = ctx.channel::<BroadcastedSensorRead>();
            sensor_channel.subscribe_to(
                ctx.myself.clone(),
                crate::sensors::actors::SENSORS_CHANNEL_NAME,
            );
        }

        fn post_stop(&mut self, _ctx: &Context<Self>) {
            debug!("Stopped adaptive brightness actor");
        }
    }

    impl<AL: Receiver<LifxCommand>> Receiver<Command> for AdaptiveBrightness<AL> {
        fn recv(&mut self, _ctx: &Context<Self>, msg: Command) {
            match msg {
                Command::TurnOn => {}
                Command::TurnOff => {}
            }
        }
    }

    impl<AL: Receiver<LifxCommand>> Receiver<BroadcastedSensorRead> for AdaptiveBrightness<AL> {
        fn recv(&mut self, _ctx: &Context<Self>, msg: BroadcastedSensorRead) {
            if !self.light_turned_on {
                debug!("light turned off, skipping sensor message");
                return;
            }

            let lux = msg.lux();
            let correction = self.controller.update(lux as f64);
            let next_bright = self.brightness_command as f64 + correction;
            trace!("next_bright after correction: {}", next_bright);

            let brightness = match next_bright.round() as i64 {
                n if n < 0 => 0,
                n if n <= std::u16::MAX as i64 => n as u16,
                _ => std::u16::MAX,
            };

            debug!("Setting brightness to {}", brightness);
            self.brightness_command = brightness;

            // TODO Make the group a field of the actor
            self.lifx.send_msg(LifxCommand::SetGroupBrightness {
                group: "Living Room - Desk".to_string(),
                brightness,
            });
            //screen.update_group_brightness("Living Room - Desk", brightness);
        }
    }
}
