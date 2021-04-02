mod configuration;
mod delay;
mod lifx;
mod mqtt;
mod screen;
mod sensors;

#[cfg(target_os = "linux")]
mod tsl_2591;

use std::sync::Arc;
use tokio::sync::RwLock;

pub(crate) use configuration::Configuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // TODO Change the .with_file_name to not have to pass a dummy file name here
    #[cfg(target_os = "linux")]
    let cfg = Configuration::from_directory("/home/pi/.mysthome/nothing")?;

    #[cfg(not(target_os = "linux"))]
    let cfg = Configuration::from_directory(
        "/Users/francoismonniot/Projects/local/my-st-home/data/project/nothing",
    )?;

    // Create our background processors (lifx, screen, mqtt, ST events)
    let s_task = mqtt::spawn(&cfg).await?;
    let (screen_join_handle, screen_handle) = screen::spawn();
    let lifx = lifx::spawn().await?;
    let sensors = sensors::spawn();

    // Dummy thing, to avoid unused warn until we have the real logic
    screen_handle.update(screen::ScreenMessage::UpdateLifxBulb {
        source: 0,
        power: true,
    })?;

    // No state persistence for now
    let light_state = Arc::new(RwLock::new(false));
    // Notify the cloud in which state we are 
    s_task.send_event(mqtt::DeviceEvent::simple_str(
        "main",
        "switch",
        "switch",
        "off",
    )).await;

    tokio::spawn(logic::adaptive_brightness(
        sensors.messages(),
        lifx.handle(),
        light_state.clone(),
    ));

    let event_sink = s_task.event_sink().await;
    tokio::spawn(logic::st_light_state(
        s_task.commands(),
        event_sink,
        lifx.handle(),
        light_state.clone(),
    ));

    // TODO Need something to trigger a stop of the various background processes
    // rust signal handling ?

    // At the end, await the end of the background processes
    sensors.join().await?;
    lifx.join_handle().await?;
    screen_join_handle.await?;
    s_task.join().await?;

    Ok(())
}

mod logic {
    use crate::{lifx::LifxHandle, mqtt::Command};
    use crate::{mqtt::DeviceEvent, sensors::SensorMessage};
    use futures::{Sink, SinkExt, Stream, StreamExt};
    use log::{debug, warn};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

    pub(super) async fn st_light_state<St, Si>(
        mut commands: St,
        events: Si,
        lifx: LifxHandle,
        light_state: Arc<RwLock<bool>>,
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

                    debug!("Received command '{}', current state is '{}'", switch, current);
                    if switch != current {
                        // Change light_state
                        {
                            let mut s = light_state.write().await;
                            *s = switch;
                        }

                        let value = if switch { "on" } else { "off" };

                        // Send device event
                        debug!("Sending device event with value '{}'", value);
                        events.send(DeviceEvent::simple_str(
                            "main",
                            "switch",
                            "switch",
                            value,
                        )).await;

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
                    debug!("next_bright after correction: {}", next_bright);

                    let brightness = match next_bright.round() as i64 {
                        n if n < 0 => 0,
                        n if n <= std::u16::MAX as i64 => n as u16,
                        _ => std::u16::MAX,
                    };

                    // let brightness = lux_to_brightness(lux);

                    debug!("Setting brightness to {}", brightness);
                    brightness_command = brightness;
                    lifx.set_group_brightness("Living Room - Desk", brightness)
                        .await;
                }
                Err(error) => {
                    warn!("Cannot pull sensor data. error={:?}", error);
                }
            }
        }

        debug!("Stopped sensors run loop");
    }
}
