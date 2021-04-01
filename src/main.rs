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

    fn lux_to_brightness(lux: f32) -> u16 {
        /*

        lux reading (night time):

        Note: light incidence is really important. The lamp was at a ~45° compared to the sensor.
        | LIFX 3500K brightness % | lux value |
        | 1                       | 1.02      |
        | 5                       | 1.5       |
        | 10                      | 2.53      |
        | 15                      | 3.56      |
        | 20                      | 5.02      |
        | 25                      | 6.68      |
        | 30                      | 8.44      |
        | 35                      | 12.23     | 16 when light is front facing (no incident)
        | 40                      | 15.16     |
        | 45                      | 18.23     |
        | 50                      | 21.89     |
        | 55                      | 25.88     |
        | 60                      | 30.08     |
        | 65                      | 35.59     |
        | 70                      | 40.33     |
        | 75                      | 46.76     |
        | 80                      | 53.05     |
        | 85                      | 58.90     |
        | 90                      | 66.32     |
        | 95                      | 74.21     |
        | 100                     | 81.13     | 132 without incidence, 70lux at 3000K
        */

        let current_lux = lux as f64;
        let target_lux: f64 = 80.0; // target is around 80 lux;

        let diff: f64 = target_lux - current_lux;

        if diff <= 0.0 {
            // current is already above the target, turn off the light
            return 0;
        }

        // See "docs/Lux curve fitting.ipynb" for details about this function
        let a = 2.6194180654551654e-004;
        let b = -4.3180054329256472e-002;
        let c = 2.9900609021127589e+000;
        let d = 3.3886006980307526e+000;

        // a*x^3 + b*x^2 + c*x + d
        let percent = a * diff.powf(3.0) + b * diff.powf(2.0) + c * diff + d;

        // The last cast is safe assume percent is bounded between 0.0 and 100.0
        ((std::u16::MAX as f64) * (percent / 100.0)) as u16
    }
}
