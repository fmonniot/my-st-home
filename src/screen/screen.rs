//! An abstraction around the actual screen:
//! - e-ink device for raspberry pi, and
//! - embedded-graphics simulator for mac os.
use std::fmt::Debug;

use embedded_hal::blocking::delay::DelayMs;

pub trait Screen {
    type Error: Debug;

    /// Let the device enter deep-sleep mode to save power.
    ///
    /// The deep sleep mode returns to standby with a hardware reset.
    fn sleep(&mut self) -> Result<(), Self::Error>;

    /// Wakes the device up from sleep
    ///
    /// Also reintialises the device if necessary.
    fn wake_up<DELAY: DelayMs<u8>>(&mut self, delay: &mut DELAY) -> Result<(), Self::Error>;

    /// Provide a combined update&display and save some time (skipping a busy check in between)
    fn update_and_display_frame(&mut self, buffer: &[u8]) -> Result<(), Self::Error>;

    /// Clears the frame buffer on the EPD with the declared background color
    ///
    /// The background color can be changed with [`WaveshareDisplay::set_background_color`]
    fn clear_frame(&mut self) -> Result<(), Self::Error>;
}

#[cfg(target_os = "linux")]
pub fn create() -> Result<impl Screen, ()> {
    let s = rasp::create();

    Ok(s)
}

#[cfg(target_os = "linux")]
mod rasp {
    use super::Screen;
    use crate::delay::Delay;
    use embedded_hal::blocking::delay::DelayMs;
    use epd_waveshare::{epd7in5_v2::EPD7in5, prelude::*};
    use rppal::gpio::{Gpio, InputPin, OutputPin};
    use rppal::spi::{Bus, Mode, SlaveSelect, Spi};

    pub struct EdpScreen {
        spi: Spi,
        screen: EPD7in5<Spi, OutputPin, InputPin, OutputPin, OutputPin>,
    }

    impl Screen for EdpScreen {
        type Error = rppal::spi::Error;

        fn sleep(&mut self) -> Result<(), Self::Error> {
            self.screen.sleep(&mut self.spi)
        }

        fn wake_up<DELAY: DelayMs<u8>>(&mut self, delay: &mut DELAY) -> Result<(), Self::Error> {
            self.screen.wake_up(&mut self.spi, delay)
        }

        fn update_and_display_frame(&mut self, buffer: &[u8]) -> Result<(), Self::Error> {
            self.screen.update_and_display_frame(&mut self.spi, buffer)
        }

        fn clear_frame(&mut self) -> Result<(), Self::Error> {
            self.screen.clear_frame(&mut self.spi)
        }
    }

    // TODO Return a Result
    pub fn create() -> EdpScreen {
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
        let epd7in5 =
            EPD7in5::new(&mut spi, cs, busy, dc, rst, &mut delay).expect("eink initalize error");

        EdpScreen {
            spi,
            screen: epd7in5,
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn create() -> Result<impl Screen, ()> {
    let s = mac::create();

    Ok(s)
}

// TODO Implement this based on the embedded-graphics simulator
#[cfg(not(target_os = "linux"))]
mod mac {
    use super::Screen;
    use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};
    use embedded_graphics_simulator::{
        BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, Window,
    };
    use embedded_hal::blocking::delay::DelayMs;
    use log::error;
    use std::sync::mpsc;
    use std::thread;
    use bitvec::prelude::*;

    pub struct MacScreen {
        join: thread::JoinHandle<()>,
        tx: mpsc::Sender<UiMessage>
    }

    /// Width of the display
    pub const WIDTH: u32 = 800;
    /// Height of the display
    pub const HEIGHT: u32 = 480;

    impl Screen for MacScreen {
        type Error = ();

        // The simulator don't got to sleep
        fn sleep(&mut self) -> Result<(), Self::Error> {
            Ok(())
        }

        // Nor does it wake up
        fn wake_up<DELAY: DelayMs<u8>>(&mut self, _delay: &mut DELAY) -> Result<(), Self::Error> {
            Ok(())
        }

        fn update_and_display_frame(&mut self, buffer: &[u8]) -> Result<(), Self::Error> {
            let size = Size::new(WIDTH, HEIGHT);
            let pixel_count = size.width as usize * size.height as usize;

            // The buffer store 8 pixels per entry (byte)
            if buffer.len() * 8 != pixel_count {
                error!("Trying to update screen with an invalide sized buffer: buffer.len = {}, expected = {}", buffer.len(), pixel_count / 8);
            }

            let mut display: SimulatorDisplay<BinaryColor> = SimulatorDisplay::new(size);


            // TODO Find out if it's Msb or Lsb (Most/Least significan bit first)
            let bits = buffer.view_bits::<Msb0>();

            for (idx, bit) in bits.iter().enumerate() {
                // Those conversion should be safe given the screen constraint we have asserted previously
                let idx = idx as u32;
                let x = (idx % WIDTH) as i32;
                let y = (idx / WIDTH) as i32;

                let point = Point::new(x, y);
                let color = if *bit {
                    BinaryColor::On
                } else {
                    BinaryColor::Off
                };
                let pixel = Pixel(point, color);

                display.draw_pixel(pixel).unwrap(); // TODO error handling
            }

            self.tx.send(UiMessage::Update(display)).unwrap(); // TODO error handling

            Ok(())
        }

        fn clear_frame(&mut self) -> Result<(), Self::Error> {
            self.tx.send(UiMessage::Clear).unwrap(); // TODO error handling
            Ok(())
        }
    }

    enum UiMessage {
        Update(SimulatorDisplay<BinaryColor>),
        Clear,
    }

    // TODO Return a Result
    pub fn create() -> MacScreen {
        let (tx, rx) = mpsc::channel::<UiMessage>();

        // SDL requires a single thread manage all operations (the "main" thread). Because we are
        // using a multi-thread async runtime, we need to create a dedicated thread to be able to
        // maintain this invarient.

        let join = thread::spawn(move || {
            // TODO Check if color theme match the e-ink device
            let output_settings = OutputSettingsBuilder::new()
                .theme(BinaryColorTheme::OledWhite)
                .build();
            let mut window = Window::new("Hello World", &output_settings);

            for message in rx {
                match message {
                    UiMessage::Update(display) => {
                        window.update(&display);
                    }
                    UiMessage::Clear => {
                        let display = SimulatorDisplay::with_default_color(
                            Size::new(WIDTH, HEIGHT),
                            BinaryColor::Off,
                        );
                        window.update(&display);
                    }
                }
            }
        });

        MacScreen { join, tx }
    }
}
