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

    // manual config, should not be const but actual config :)
    const MIN_LUX: f32 = 240.0;

    pub(super) async fn sensors<S>(mut sensors: S, lifx: LifxHandle)
    where
        S: Stream<Item = Result<SensorMessage, BroadcastStreamRecvError>> + Unpin,
    {
        debug!("Starting sensors run loop");

        while let Some(message) = sensors.next().await {
            match message {
                Ok(SensorMessage::Luminosity { lux, .. }) => {

                    // It's bright enough already. Nothing to do.
                    if lux >= MIN_LUX {
                        continue;
                    }

                    /*
                        Need a table form lux reading to lifx's brightness level
                        1% in the app gave:
                        HSBK { hue: 7461, saturation: 0, brightness: 1966, kelvin: 3500 }

                        100% in the app gave:
                        HSBK { hue: 7461, saturation: 0, brightness: 65535, kelvin: 3500 }

                        So HSBK.brightness is the only interesting part, and it goes from 0 to 65535 (16 bytes)
                     */

                     // TODO wait when it's dark enough that I can find the actual lux reading :)
                    
                }
                Err(error) => {
                    warn!("Cannot pull sensor data. error={:?}", error);
                }
            }
        }

        debug!("Stopped sensors run loop");
    }
}
