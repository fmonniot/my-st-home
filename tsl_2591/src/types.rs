use std::error;
use std::fmt;

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
    pub(super) fn from_u8(gain: u8) -> Gain {
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
    pub(super) fn from_u8(integration_time: u8) -> IntegrationTime {
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

// TODO Error type parameter
/// Errors when accessing the sensor
#[derive(Debug)]
pub enum TSL2591Error<E: fmt::Debug> {
    /// Errors that occur when accessing the I2C peripheral.
    I2cError(E),
    /// Overflow error when calculating lux
    OverflowError,
    /// Error throw when the sensor is not found
    RuntimeError,
}

impl<E: fmt::Display + fmt::Debug> fmt::Display for TSL2591Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            TSL2591Error::RuntimeError => write!(f, "Unable to find device TSL2591"),
            TSL2591Error::OverflowError => write!(f, "Overflow reading light channels"),
            TSL2591Error::I2cError(ref err) => write!(f, "i2cError: {}", err),
        }
    }
}

impl<E: fmt::Display + fmt::Debug> error::Error for TSL2591Error<E> {}

impl<E: fmt::Debug> From<E> for TSL2591Error<E> {
    fn from(err: E) -> TSL2591Error<E> {
        TSL2591Error::I2cError(err)
    }
}
