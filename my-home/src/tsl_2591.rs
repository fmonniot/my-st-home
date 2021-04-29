//! This is largely a port of
//! https://github.com/adafruit/Adafruit_CircuitPython_TSL2591/blob/master/adafruit_tsl2591.py
//! https://github.com/robbielamb/tsl2591_sensor/blob/master/src/tsl2591_sensor.rs
// TODO Should probably rewritten on top of embedded-hal instead
// https://github.com/eldruin/veml6030-rs for inspiration on arch/structure.
use rppal::i2c;
use std::error;
use std::fmt;

const TSL2591_ADDR: u16 = 0x29;
const TSL2591_COMMAND_BIT: u8 = 0xA0;

const TSL2591_ENABLE_POWEROFF: u8 = 0x00;
const TSL2591_ENABLE_POWERON: u8 = 0x01;
const TSL2591_ENABLE_AEN: u8 = 0x02;
const TSL2591_ENABLE_AIEN: u8 = 0x10;
const TSL2591_ENABLE_NPIEN: u8 = 0x80;
const TSL2591_REGISTER_ENABLE: u8 = 0x00;
const TSL2591_REGISTER_CONTROL: u8 = 0x01;
const TSL2591_REGISTER_DEVICE_ID: u8 = 0x12;
const TSL2591_REGISTER_CHAN0_LOW: u8 = 0x14;
const TSL2591_REGISTER_CHAN1_LOW: u8 = 0x16;

// Numbers for calculating LUX
const TSL2591_LUX_DF: f32 = 408.0;
const TSL2591_LUX_COEFB: f32 = 1.64;
const TSL2591_LUX_COEFC: f32 = 0.59;
const TSL2591_LUX_COEFD: f32 = 0.86;

const TSL2591_MAX_COUNT_100MS: u16 = 0x8FFF;
const TSL2591_MAX_COUNT: u16 = 0xFFFF;

/// Available Gains for the sensor
#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum Gain {
    /// gain of 1x
    LOW = 0x00, // 1x
    /// gain of 25x
    MED = 0x10, // 25x
    /// gain of 428x
    HIGH = 0x20, // 428x
    /// gain of 9876x
    MAX = 0x30, // 9876x
}

impl Gain {
    fn from_u8(gain: u8) -> Gain {
        match gain {
            0x00 => Gain::LOW,
            0x10 => Gain::MED,
            0x20 => Gain::HIGH,
            0x30 => Gain::MAX,
            _ => panic!("Bad U89"),
        }
    }
}

impl fmt::Display for Gain {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Gain::LOW => write!(f, "LOW (1x)"),
            Gain::MED => write!(f, "MED (25x)"),
            Gain::HIGH => write!(f, "HIGH (428x)"),
            Gain::MAX => write!(f, "MAX (9876x)"),
        }
    }
}

/// Available integration times for the sensor
#[repr(u8)]
#[derive(Copy, Clone, Debug)]
pub enum IntegrationTime {
    /// 100ms integration time
    Time100ms = 0x00,
    /// 200ms integration time
    Time200ms = 0x01,
    /// 300ms integration time
    Time300ms = 0x02,
    /// 400ms integration time
    Time400ms = 0x03,
    /// 500ms integration time
    Time500ms = 0x04,
    /// 600ms integration time
    Time600ms = 0x05,
}

impl IntegrationTime {
    fn from_u8(integration_time: u8) -> IntegrationTime {
        match integration_time {
            0x00 => IntegrationTime::Time100ms,
            0x01 => IntegrationTime::Time200ms,
            0x02 => IntegrationTime::Time300ms,
            0x03 => IntegrationTime::Time400ms,
            0x04 => IntegrationTime::Time500ms,
            0x05 => IntegrationTime::Time600ms,
            _ => panic!("Integration time out of range"),
        }
    }
}

impl fmt::Display for IntegrationTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            IntegrationTime::Time100ms => write!(f, "100ms"),
            IntegrationTime::Time200ms => write!(f, "200ms"),
            IntegrationTime::Time300ms => write!(f, "300ms"),
            IntegrationTime::Time400ms => write!(f, "400ms"),
            IntegrationTime::Time500ms => write!(f, "500ms"),
            IntegrationTime::Time600ms => write!(f, "600ms"),
        }
    }
}

/// Errors when accessing the sensor
#[derive(Debug)]
pub enum TSL2591Error {
    /// Errors that occur when accessing the I2C peripheral.
    I2cError(i2c::Error),
    /// Overflow error when calculating lux
    OverflowError,
    /// Error throw when the sensor is not found
    RuntimeError,
}

impl fmt::Display for TSL2591Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TSL2591Error::RuntimeError => write!(f, "Unable to find device TSL2591"),
            TSL2591Error::OverflowError => write!(f, "Overflow reading light channels"),
            TSL2591Error::I2cError(ref err) => write!(f, "i2cError: {}", err),
        }
    }
}

impl error::Error for TSL2591Error {}

impl From<i2c::Error> for TSL2591Error {
    fn from(err: i2c::Error) -> TSL2591Error {
        TSL2591Error::I2cError(err)
    }
}

/// Provide access to a TSL2591 sensor on the i2c bus.
#[derive(Debug)]
pub struct TSL2591Sensor {
    i2cbus: i2c::I2c,
    gain: Gain,
    integration_time: IntegrationTime,
}

impl TSL2591Sensor {
    /// Construct a new TSL2591 sensor on the given i2c bus.
    ///
    /// The device is returned enabled and ready to use.
    /// The gain and integration times can be changed after returning.
    pub fn new(i2cbus: i2c::I2c) -> Result<TSL2591Sensor, TSL2591Error> {
        let mut obj = TSL2591Sensor {
            i2cbus,
            gain: Gain::MED,
            integration_time: IntegrationTime::Time100ms,
        };

        obj.i2cbus.set_slave_address(TSL2591_ADDR)?;

        if obj.read_u8(TSL2591_REGISTER_DEVICE_ID)? != 0x50 {
            return Err(TSL2591Error::RuntimeError);
        }

        obj.set_gain(obj.gain)?;
        obj.set_integration_time(obj.integration_time)?;

        obj.enable()?;

        Ok(obj)
    }

    /// Read a byte from the given address
    fn read_u8(&self, address: u8) -> Result<u8, TSL2591Error> {
        let command = (TSL2591_COMMAND_BIT | address) & 0xFF;
        Ok(self.i2cbus.smbus_read_byte(command)?)
    }

    /// Write a byte to the given address
    fn write_u8(&self, address: u8, val: u8) -> Result<(), TSL2591Error> {
        let command = (TSL2591_COMMAND_BIT | address) & 0xFF;
        let val = val & 0xFF;
        self.i2cbus.smbus_write_byte(command, val)?;
        Ok(())
    }

    /// Read a word from the given address
    fn read_u16(&self, address: u8) -> Result<u16, TSL2591Error> {
        let command = (TSL2591_COMMAND_BIT | address) & 0xFF;
        Ok(self.i2cbus.smbus_read_word(command)?)
    }

    /// Set the Gain for the sensor
    pub fn set_gain(&mut self, gain: Gain) -> Result<(), TSL2591Error> {
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        let updated_control = (control & 0b11001111) | (gain as u8);

        self.write_u8(TSL2591_REGISTER_CONTROL, updated_control)?;
        self.gain = gain;
        Ok(())
    }

    /// Get the current configred gain on the sensor
    pub fn get_gain(&self) -> Result<Gain, TSL2591Error> {
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        Ok(Gain::from_u8(control & 0b00110000))
        // Check to see if the saved value is what we pulled back?
    }

    /// Set the integration time on the sensor
    pub fn set_integration_time(
        &mut self,
        integration_time: IntegrationTime,
    ) -> Result<(), TSL2591Error> {
        self.integration_time = integration_time;
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        let updated_control = (control & 0b11111000) | (integration_time as u8);
        self.write_u8(TSL2591_REGISTER_CONTROL, updated_control)
    }

    /// Get the current integration time configured on the sensor
    pub fn get_integration_time(&self) -> Result<IntegrationTime, TSL2591Error> {
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        Ok(IntegrationTime::from_u8(control & 0b0000111))
    }

    /// Activate the device using all the features.
    pub fn enable(&self) -> Result<(), TSL2591Error> {
        let command = TSL2591_ENABLE_POWERON
            | TSL2591_ENABLE_AEN
            | TSL2591_ENABLE_AIEN
            | TSL2591_ENABLE_NPIEN;
        self.write_u8(TSL2591_REGISTER_ENABLE, command)
    }

    /// Disables the device putting it in a low power mode.
    pub fn disable(&self) -> Result<(), TSL2591Error> {
        self.write_u8(TSL2591_REGISTER_ENABLE, TSL2591_ENABLE_POWEROFF)
    }

    /// Read the raw values from the sensor and return a tuple
    /// The first channel is is IR+Visible luminosity and the
    /// second is IR only.
    fn raw_luminosity(&self) -> Result<(u16, u16), TSL2591Error> {
        let channel0 = self.read_u16(TSL2591_REGISTER_CHAN0_LOW)?;
        let channel1 = self.read_u16(TSL2591_REGISTER_CHAN1_LOW)?;

        Ok((channel0, channel1))
    }

    /// Read the full spectrum (IR+Visible)
    pub fn full_spectrum(&self) -> Result<u32, TSL2591Error> {
        let (chan0, chan1) = self.raw_luminosity()?;
        let chan0 = chan0 as u32;
        let chan1 = chan1 as u32;
        Ok(chan1 << 16 | chan0)
    }

    /// Read the infrared light
    pub fn infrared(&self) -> Result<u16, TSL2591Error> {
        let (_, chan1) = self.raw_luminosity()?;
        Ok(chan1)
    }

    /// Read the visible light
    pub fn visible(&self) -> Result<u32, TSL2591Error> {
        let (chan0, chan1) = self.raw_luminosity()?;
        let chan0 = chan0 as u32;
        let chan1 = chan1 as u32;
        let full = (chan1 << 16) | chan0;
        Ok(full - chan1)
    }

    /// Read the sensor and compute a lux value.
    /// There are many opinions on computing lux values.
    /// See:
    /// https://github.com/adafruit/Adafruit_CircuitPython_TSL2591/blob/master/adafruit_tsl2591.py
    /// https://github.com/adafruit/Adafruit_TSL2591_Library/blob/master/Adafruit_TSL2591.cpp
    pub fn lux(&self) -> Result<f32, TSL2591Error> {
        let (chan0, chan1) = self.raw_luminosity()?;

        // Compute the atime in milliseconds
        let atime = 100.0 * (self.integration_time as u8 as f32) + 100.0;

        let max_counts = match self.integration_time {
            IntegrationTime::Time100ms => TSL2591_MAX_COUNT_100MS,
            _ => TSL2591_MAX_COUNT,
        };

        if chan0 >= max_counts || chan1 >= max_counts {
            return Err(TSL2591Error::OverflowError);
        };

        let again = match self.gain {
            Gain::LOW => 1.0,
            Gain::MED => 25.0,
            Gain::HIGH => 428.0,
            Gain::MAX => 9876.0,
        };

        let cp1 = (atime * again) / TSL2591_LUX_DF;
        let lux1 = (chan0 as f32 - (TSL2591_LUX_COEFB * chan0 as f32)) / cp1;
        let lux2 = ((TSL2591_LUX_COEFC * chan0 as f32) - (TSL2591_LUX_COEFD * chan1 as f32)) / cp1;

        Ok(lux1.max(lux2))
    }
}
