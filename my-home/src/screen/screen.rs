//! An abstraction around the actual screen:
//! - e-ink device for raspberry pi, and
//! - embedded-graphics simulator for mac os.

#[cfg(target_os = "linux")]
pub use rasp::Screen;

#[cfg(not(target_os = "linux"))]
pub use mac::Screen;

#[cfg(target_os = "linux")]
pub fn create() -> Result<Screen, ()> {
    let s = rasp::create();

    Ok(s)
}

#[cfg(not(target_os = "linux"))]
pub fn create() -> Result<Screen, ()> {
    let s = mac::create();

    Ok(s)
}

#[cfg(target_os = "linux")]
mod rasp {
    use super::super::delay::Delay;
    use epd_waveshare::{epd7in5_v2::Epd7in5, prelude::*};
    use rppal::gpio::{Gpio, InputPin, OutputPin};
    use rppal::spi::{Bus, Error, Mode, SlaveSelect, Spi};

    pub struct Screen {
        spi: Spi,
        screen: Epd7in5<Spi, OutputPin, InputPin, OutputPin, OutputPin, Delay>,
        delay: Delay,
    }

    impl Screen {
        pub fn sleep(&mut self) -> Result<(), Error> {
            self.screen.sleep(&mut self.spi, &mut self.delay)
        }

        pub fn wake_up(&mut self) -> Result<(), Error> {
            self.screen.wake_up(&mut self.spi, &mut self.delay)
        }

        pub fn update_and_display_frame(&mut self, buffer: &[u8]) -> Result<(), Error> {
            self.screen
                .update_and_display_frame(&mut self.spi, buffer, &mut self.delay)
        }
    }

    // TODO Return a Result
    pub fn create() -> Screen {
        // Configure SPI and GPIO
        let mut spi =
            Spi::new(Bus::Spi0, SlaveSelect::Ss0, 4_000_000, Mode::Mode0).expect("spi bus");

        let gpio = Gpio::new().expect("gpio");
        let cs = gpio.get(8).expect("CS").into_output();
        let busy = gpio.get(24).expect("BUSY").into_input();
        let dc = gpio.get(25).expect("DC").into_output();
        let rst = gpio.get(17).expect("RST").into_output();

        let mut delay = Delay {};

        // Configure the screen before creating the run loop
        let epd7in5 = Epd7in5::new(&mut spi, cs, busy, dc, rst, &mut delay, Some(1))
            .expect("eink initalize error");

        Screen {
            spi,
            screen: epd7in5,
            delay,
        }
    }
}

// TODO Implement this based on the embedded-graphics simulator
#[cfg(not(target_os = "linux"))]
mod mac {
    pub struct Screen {}

    impl Screen {
        // The simulator don't got to sleep
        pub fn sleep(&mut self) -> Result<(), ()> {
            Ok(())
        }

        // Nor does it wake up
        pub fn wake_up(&mut self) -> Result<(), ()> {
            Ok(())
        }

        pub fn update_and_display_frame(&mut self, _buffer: &[u8]) -> Result<(), ()> {
            Ok(())
        }
    }

    pub fn create() -> Screen {
        Screen {}
    }
}
