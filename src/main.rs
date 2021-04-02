mod configuration;
mod delay;
mod lifx;
mod mqtt;
mod screen;
mod sensors;

#[cfg(target_os = "linux")]
mod tsl_2591;

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
    let _stdk = if false {
        mqtt::spawn(&cfg).await?
    } else {
    };
    let (screen_join_handle, screen_handle) = screen::spawn();
    let lifx = lifx::spawn().await?;
    let sensors = sensors::spawn();

    // Then create the handle to use in the business loop
    let _lifx_handle = lifx.handle();

    // Dummy thing, to avoid unused warn until we have the real logic
    screen_handle.update(screen::ScreenMessage::UpdateLifxBulb {
        source: 0,
        power: true,
    })?;

    tokio::spawn(logic::sensors(sensors.messages(), lifx.handle()));

    // TODO Need something to trigger a stop of the various background processes
    // rust signal handling ?

    // At the end, await the end of the background processes
    sensors.join().await?;
    lifx.join_handle().await?;
    screen_join_handle.await?;

    Ok(())
}

mod logic {
    use crate::lifx::LifxHandle;
    use crate::sensors::SensorMessage;
    use futures::{Stream, StreamExt};
    use log::{debug, warn};
    use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

    pub(super) async fn sensors<S>(mut sensors: S, lifx: LifxHandle)
    where
        S: Stream<Item = Result<SensorMessage, BroadcastStreamRecvError>> + Unpin,
    {
        debug!("Starting sensors run loop");

        // TODO Needs to make that configurable without code change
        let mut controller = pid_lite::Controller::new(80.0, 100.0, 0.01, 0.01);
        let mut brightness_command: u16 = 1000;

        // TODO Needs some form of better control loop (PI, hysteresis, other ?)
        // The naive approach will just turn on/off the lamps whenever the brightness setting is too big
        // instead of adjusting it.
        while let Some(message) = sensors.next().await {
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
