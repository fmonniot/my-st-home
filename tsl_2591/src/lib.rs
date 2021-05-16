//! This is largely a port of
//! https://github.com/adafruit/Adafruit_CircuitPython_TSL2591/blob/master/adafruit_tsl2591.py
//! https://github.com/robbielamb/tsl2591_sensor/blob/master/src/tsl2591_sensor.rs
use std::fmt::{Debug, Display};

// TODO Should probably rewritten on top of embedded-hal instead
// https://github.com/eldruin/veml6030-rs for inspiration on arch/structure.
// TODO Not no_std at the time because of std::error::Error. It should be easy
// to remove them and we can go no_std. Note that means my-home will probably need
// a newtype around that error to be able to still work with Error.
use embedded_hal::blocking::i2c;

mod types;
pub use types::{Gain, IntegrationTime, TSL2591Error};

const TSL2591_ADDR: u8 = 0x29;
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

/// Provide access to a TSL2591 sensor on the i2c bus.
#[derive(Debug)]
pub struct TSL2591Sensor<I2C> {
    i2c: I2C,
    gain: Gain,
    integration_time: IntegrationTime,
}

// TODO impl separated in 3 or 4 sections: no constraints, write, read, write+read
impl<I2C, E> TSL2591Sensor<I2C>
where
    I2C: i2c::WriteRead<Error = E> + i2c::Write<Error = E>,
    E: Debug + Display,
{
    /// Construct a new TSL2591 sensor on the given i2c bus.
    ///
    /// The device is returned enabled and ready to use.
    /// The gain and integration times can be changed after returning.
    pub fn new(i2c: I2C) -> Result<TSL2591Sensor<I2C>, TSL2591Error<E>> {
        let mut obj = TSL2591Sensor {
            i2c,
            gain: Gain::MED,
            integration_time: IntegrationTime::Time100ms,
        };

        if obj.read_u8(TSL2591_REGISTER_DEVICE_ID)? != 0x50 {
            return Err(TSL2591Error::RuntimeError);
        }

        obj.set_gain(obj.gain)?;
        obj.set_integration_time(obj.integration_time)?;

        obj.enable()?;

        Ok(obj)
    }

    /// Destroy driver instance, return I²C bus instance.
    pub fn destroy(self) -> I2C {
        self.i2c
    }

    // Abstraction over the raw traits

    /// Read a byte from the given register
    fn read_u8(&mut self, register: u8) -> Result<u8, TSL2591Error<E>> {
        let mut data = [0; 1];
        self.i2c.write_read(
            TSL2591_ADDR,
            &[(TSL2591_COMMAND_BIT | register) & 0xFF],
            &mut data,
        )?;

        Ok(data[0])
    }

    /// Read a word from the given register
    fn read_u16(&mut self, register: u8) -> Result<u16, TSL2591Error<E>> {
        let mut data = [0; 2];
        self.i2c.write_read(
            TSL2591_ADDR,
            &[(TSL2591_COMMAND_BIT | register) & 0xFF],
            &mut data,
        )?;

        Ok(u16::from(data[0]) | u16::from(data[1]) << 8)
    }

    /// Write a byte to the given address
    fn write_u8(&mut self, address: u8, val: u8) -> Result<(), TSL2591Error<E>> {
        let command = (TSL2591_COMMAND_BIT | address) & 0xFF;
        let val = val & 0xFF;
        //self.i2cbus.smbus_write_byte(command, val)?;

        self.i2c.write(TSL2591_ADDR, &[command, val])?;

        Ok(())
    }

    /// Set the Gain for the sensor
    pub fn set_gain(&mut self, gain: Gain) -> Result<(), TSL2591Error<E>> {
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        let updated_control = (control & 0b11001111) | (gain as u8);

        self.write_u8(TSL2591_REGISTER_CONTROL, updated_control)?;
        self.gain = gain;
        Ok(())
    }

    /// Get the current configred gain on the sensor
    pub fn get_gain(&mut self) -> Result<Gain, TSL2591Error<E>> {
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        Ok(Gain::from_u8(control & 0b00110000))
        // Check to see if the saved value is what we pulled back?
    }

    /// Set the integration time on the sensor
    pub fn set_integration_time(
        &mut self,
        integration_time: IntegrationTime,
    ) -> Result<(), TSL2591Error<E>> {
        self.integration_time = integration_time;
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        let updated_control = (control & 0b11111000) | (integration_time as u8);
        self.write_u8(TSL2591_REGISTER_CONTROL, updated_control)
    }

    /// Get the current integration time configured on the sensor
    pub fn get_integration_time(&mut self) -> Result<IntegrationTime, TSL2591Error<E>> {
        let control = self.read_u8(TSL2591_REGISTER_CONTROL)?;
        Ok(IntegrationTime::from_u8(control & 0b0000111))
    }

    /// Activate the device using all the features.
    pub fn enable(&mut self) -> Result<(), TSL2591Error<E>> {
        let command = TSL2591_ENABLE_POWERON
            | TSL2591_ENABLE_AEN
            | TSL2591_ENABLE_AIEN
            | TSL2591_ENABLE_NPIEN;
        self.write_u8(TSL2591_REGISTER_ENABLE, command)
    }

    /// Disables the device putting it in a low power mode.
    pub fn disable(&mut self) -> Result<(), TSL2591Error<E>> {
        self.write_u8(TSL2591_REGISTER_ENABLE, TSL2591_ENABLE_POWEROFF)
    }

    /// Read the raw values from the sensor and return a tuple
    /// The first channel is is IR+Visible luminosity and the
    /// second is IR only.
    fn raw_luminosity(&mut self) -> Result<(u16, u16), TSL2591Error<E>> {
        let channel0 = self.read_u16(TSL2591_REGISTER_CHAN0_LOW)?;
        let channel1 = self.read_u16(TSL2591_REGISTER_CHAN1_LOW)?;

        Ok((channel0, channel1))
    }

    /// Read the full spectrum (IR+Visible)
    pub fn full_spectrum(&mut self) -> Result<u32, TSL2591Error<E>> {
        let (chan0, chan1) = self.raw_luminosity()?;
        let chan0 = chan0 as u32;
        let chan1 = chan1 as u32;
        Ok(chan1 << 16 | chan0)
    }

    /// Read the infrared light
    pub fn infrared(&mut self) -> Result<u16, TSL2591Error<E>> {
        let (_, chan1) = self.raw_luminosity()?;
        Ok(chan1)
    }

    /// Read the visible light
    pub fn visible(&mut self) -> Result<u32, TSL2591Error<E>> {
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
    pub fn lux(&mut self) -> Result<f32, TSL2591Error<E>> {
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
