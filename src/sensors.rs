use futures::Stream;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

pub struct SensorsTask {
    sender: broadcast::Sender<SensorMessage>,
    handle: JoinHandle<()>,
}

// Rename to task and follow the LIFX model ?
impl SensorsTask {
    // TODO add a way to access a stream

    pub fn messages(
        &self,
    ) -> impl Stream<
        Item = Result<SensorMessage, tokio_stream::wrappers::errors::BroadcastStreamRecvError>,
    > {
        tokio_stream::wrappers::BroadcastStream::new(self.sender.subscribe())
    }

    pub async fn join(self) -> Result<(), tokio::task::JoinError> {
        self.handle.await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum SensorMessage {
    Luminosity {
        visible: u32,
        infrared: u16,
        full_spectrum: u32,
        lux: f32,
    },
}

pub fn spawn() -> SensorsTask {
    // Then create the communication channel
    let (sender, _) = broadcast::channel(5);

    let handle = runloop::create(sender.clone());

    SensorsTask { sender, handle }
}

#[cfg(target_os = "linux")]
mod runloop {
    use super::SensorMessage;
    use crate::tsl_2591::TSL2591Sensor;
    use log::{debug, trace, warn};
    use rppal::i2c::I2c;
    use tokio::task::JoinHandle;

    pub(super) fn create(sender: tokio::sync::broadcast::Sender<SensorMessage>) -> JoinHandle<()> {
        let i2c = I2c::new().expect("Unable to open I2C bus.");
        let lux_dev = TSL2591Sensor::new(i2c).expect("Unable to open sensor device.");

        debug!(
            "Gain is: {}",
            lux_dev.get_gain().expect("Unable to get gain")
        );
        debug!(
            "Integration time is: {}",
            lux_dev
                .get_integration_time()
                .expect("Unable to get integration time")
        );

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;

                let visible = lux_dev.visible().unwrap();
                let infrared = lux_dev.infrared().unwrap();
                let full_spectrum = lux_dev.full_spectrum().unwrap();
                let lux = lux_dev.lux().unwrap();
                trace!("Visible: {}", visible);
                trace!("Infrared: {}", infrared);
                trace!("Full Spectrum: {}", full_spectrum);
                trace!("Lux: {}", lux);

                match sender.send(SensorMessage::Luminosity {
                    visible,
                    infrared,
                    full_spectrum,
                    lux,
                }) {
                    Ok(_) => (),
                    Err(e) => warn!("Failed to send sensor message: {:?}", e),
                };
            }
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod runloop {
    pub(super) fn create(
        _sender: tokio::sync::broadcast::Sender<super::SensorMessage>,
    ) -> tokio::task::JoinHandle<()> {
        log::warn!("No sensors available on this target. No data will be captured.");
        tokio::spawn(async move {})
    }
}
