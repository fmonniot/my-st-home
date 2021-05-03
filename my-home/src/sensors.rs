
use crate::actor::{Actor, ChannelRef, Context, Message, Receiver, Timer};
use log::trace;
use std::time::Duration;

/// Channel name where [`BroadcastedSensorRead`] are published to.
pub const SENSORS_CHANNEL_NAME: &str = "sensors/luminosity";

#[derive(Debug, Clone)]
pub struct BroadcastedSensorRead(Luminosity);

impl BroadcastedSensorRead {
    pub fn lux(&self) -> f32 {
        self.0.lux
    }
}

#[derive(Debug, Clone)]
pub enum SensorsMessage {
    ReadRequest,
}

impl Message for SensorsMessage {}
impl Message for BroadcastedSensorRead {}

/// An actor which regularly poll the various onboard sensors and publish
/// the results to a known channel for other actors to consume.
#[derive(Default)]
pub struct Sensors {
    channel: Option<ChannelRef<BroadcastedSensorRead>>, // type is probably wrong
    reader: Option<implementation::Reader>,
}

impl Actor for Sensors {
    fn pre_start(&mut self, ctx: &Context<Self>) {
        self.channel = Some(ctx.channel());
        self.reader = Some(implementation::reader());

        let delay = Duration::from_secs(5);

        // Set up a request to read sensors value every 5 seconds.
        // Ignore the schedule id. Only needed if we need to cancel the schedule.
        let _ = ctx.schedule(
            delay,
            delay,
            ctx.myself.clone(),
            SensorsMessage::ReadRequest,
        );
    }

    fn post_stop(&mut self, _ctx: &Context<Self>) {
        self.channel = None;
        self.reader = None;
    }
}

impl Receiver<SensorsMessage> for Sensors {
    fn recv(&mut self, _ctx: &Context<Self>, msg: SensorsMessage) {
        match msg {
            SensorsMessage::ReadRequest => {
                if let Some(reader) = &self.reader {
                    let luminosity = reader.read();
                    trace!("Reading value {:?} from sensor", luminosity);

                    if let Some(channel) = &self.channel {
                        channel.publish(BroadcastedSensorRead(luminosity), SENSORS_CHANNEL_NAME)
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Luminosity {
    visible: u32,
    infrared: u16,
    full_spectrum: u32,
    lux: f32,
}

trait LuminosityReader {
    fn read(&self) -> Luminosity; // todo errors
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

#[cfg(target_os = "linux")]
mod implementation {
    use super::{Luminosity, SensorMessage};
    use crate::tsl_2591::TSL2591Sensor;
    use log::{debug, trace, warn};
    use rppal::i2c::I2c;
    use tokio::task::JoinHandle;

    pub(super) struct Reader {
        lux_dev: TSL2591Sensor,
    }

    pub(super) fn reader() -> Reader {
        let i2c = I2c::new().expect("Unable to open I2C bus.");
        let lux_dev = TSL2591Sensor::new(i2c).expect("Unable to open sensor device.");

        Reader { lux_dev }
    }

    impl super::LuminosityReader for Reader {
        fn read(&self) -> Luminosity {
            let visible = self.lux_dev.visible().unwrap();
            let infrared = self.lux_dev.infrared().unwrap();
            let full_spectrum = self.lux_dev.full_spectrum().unwrap();
            let lux = self.lux_dev.lux().unwrap();

            Luminosity {
                visible,
                infrared,
                full_spectrum,
                lux,
            }
        }
    }
}

// TODO Once fully migrated to actor model, rename to something else.
// maybe `platform` ?
#[cfg(not(target_os = "linux"))]
mod implementation {

    use super::Luminosity;

    pub(super) struct Reader {
        // random_generator
    }

    pub(super) fn reader() -> Reader {
        Reader {}
    }

    impl super::LuminosityReader for Reader {
        fn read(&self) -> Luminosity {
            let visible = 1;
            let infrared = 2;
            let full_spectrum = 3;
            let lux = 4.0;

            Luminosity {
                visible,
                infrared,
                full_spectrum,
                lux,
            }
        }
    }
}
