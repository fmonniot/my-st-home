//! An abstraction around the actual screen:
//! - e-ink device for raspberry pi, and
//! - embedded-graphics simulator for mac os.

#[cfg(target_os = "linux")]
pub use rasp::Screen;

#[cfg(not(target_os = "linux"))]
pub use mac::Screen;

#[cfg(target_os = "linux")]
pub fn create() -> Result<(Screen, impl FnOnce() -> ()), ()> {
    let s = rasp::create();

    Ok(s)
}

#[cfg(not(target_os = "linux"))]
pub fn create() -> Result<(Screen, impl FnOnce() -> ()), ()> {
    let s = mac::create();

    Ok(s)
}

#[cfg(target_os = "linux")]
mod rasp {
    use crate::delay::Delay;
    use embedded_hal::blocking::delay::DelayMs;
    use epd_waveshare::{epd7in5_v2::EPD7in5, prelude::*};
    use rppal::gpio::{Gpio, InputPin, OutputPin};
    use rppal::spi::{Bus, Error, Mode, SlaveSelect, Spi};

    pub struct Screen {
        spi: Spi,
        screen: EPD7in5<Spi, OutputPin, InputPin, OutputPin, OutputPin>,
    }

    impl Screen {
        pub fn sleep(&mut self) -> Result<(), Error> {
            self.screen.sleep(&mut self.spi)
        }

        pub fn wake_up<DELAY: DelayMs<u8>>(&mut self, delay: &mut DELAY) -> Result<(), Error> {
            self.screen.wake_up(&mut self.spi, delay)
        }

        pub fn update_and_display_frame(&mut self, buffer: &[u8]) -> Result<(), Error> {
            self.screen.update_and_display_frame(&mut self.spi, buffer)
        }

        pub fn clear_frame(&mut self) -> Result<(), Error> {
            self.screen.clear_frame(&mut self.spi)
        }
    }

    // TODO Return a Result
    pub fn create() -> (Screen, impl FnOnce() -> ()) {
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

        let runloop = || {};

        (
            Screen {
                spi,
                screen: epd7in5,
            },
            runloop,
        )
    }
}

// TODO Implement this based on the embedded-graphics simulator
#[cfg(not(target_os = "linux"))]
mod mac {
    use bitvec::prelude::*;
    use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};
    use embedded_graphics_simulator::{
        BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, Window,
    };
    use embedded_hal::blocking::delay::DelayMs;
    use log::{debug, error, info};
    use std::{sync::mpsc, time::Duration};

    pub struct Screen {
        tx: mpsc::Sender<UiMessage>,
    }

    impl Screen {
        // The simulator don't got to sleep
        pub fn sleep(&mut self) -> Result<(), ()> {
            Ok(())
        }

        // Nor does it wake up
        pub fn wake_up<DELAY: DelayMs<u8>>(&mut self, _delay: &mut DELAY) -> Result<(), ()> {
            Ok(())
        }

        pub fn update_and_display_frame(&mut self, buffer: &[u8]) -> Result<(), ()> {
            self.tx.send(UiMessage::Update(buffer.to_vec())).unwrap(); // TODO error handling
            Ok(())
        }

        pub fn clear_frame(&mut self) -> Result<(), ()> {
            self.tx.send(UiMessage::Clear).unwrap(); // TODO error handling
            Ok(())
        }
    }

    enum UiMessage {
        Update(Vec<u8>),
        Clear,
    }

    impl std::fmt::Debug for UiMessage {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                UiMessage::Clear => write!(f, "Clear"),
                UiMessage::Update(_) => write!(f, "Update"),
            }
        }
    }

    /// Width of the display
    pub const WIDTH: u32 = 800;
    /// Height of the display
    pub const HEIGHT: u32 = 480;

    // TODO Return a Result
    pub fn create() -> (Screen, impl FnOnce() -> ()) {
        let (tx, rx) = mpsc::channel::<UiMessage>();

        // SDL requires a single thread manage all operations (the "main" thread). So we have
        // to run this on the main thread (in main)
        let runloop = move || {
            // TODO Check if color theme match the e-ink device
            info!("Initialize window");
            let output_settings = OutputSettingsBuilder::new()
                .scale(1) // Assuming macos scale is the same everywhere :)
                .theme(BinaryColorTheme::OledWhite)
                .build();
            let mut window = Window::new("My ST Home", &output_settings);

            // Create a new simulator display with 800x480 pixels.
            let mut display: SimulatorDisplay<BinaryColor> =
                SimulatorDisplay::new(Size::new(WIDTH, HEIGHT));

            window.update(&display);
            // Consume the events
            window.events().fold((), |_, _| ());

            'ui: loop {
                // We have to check the window events so that it doesn't freeze
                let next = rx.recv_timeout(Duration::from_millis(20));

                match next {
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => (),
                    Ok(message) => {
                        debug!("Received a UI message: {:?}", message);
                        match message {
                            UiMessage::Update(buffer) => {
                                display.clear(BinaryColor::Off).unwrap();
                                buffer_to_display(buffer, &mut display);
                                window.update(&display);
                            }
                            UiMessage::Clear => {
                                display.clear(BinaryColor::Off).unwrap();
                                window.update(&display);
                            }
                        }
                    }
                };

                // Consume the events
                for e in window.events() {
                    match e {
                        embedded_graphics_simulator::SimulatorEvent::Quit => break 'ui,
                        _ => (),
                    }
                }
            }

            info!("Window run loop ended");
        };

        (Screen { tx }, runloop)
    }

    fn buffer_to_display(buffer: Vec<u8>, display: &mut SimulatorDisplay<BinaryColor>) {
        let size = Size::new(WIDTH, HEIGHT);
        let pixel_count = size.width as usize * size.height as usize;

        // The buffer store 8 pixels per entry (byte)
        if buffer.len() * 8 != pixel_count {
            error!("Trying to update screen with an invalide sized buffer: buffer.len = {}, expected = {}", buffer.len(), pixel_count / 8);
        }

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
    }
}
