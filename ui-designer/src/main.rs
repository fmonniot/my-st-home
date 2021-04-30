use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, SimulatorEvent, Window,
};
use log::{debug, info};
use my_home::screen::Frame;
use sdl2::keyboard::Keycode;

/// Width of the display
const WIDTH: u32 = 800;
/// Height of the display
const HEIGHT: u32 = 480;

fn main() {
    env_logger::init();

    // TODO Check if color theme match the e-ink device
    info!("Initialize window");
    let output_settings = OutputSettingsBuilder::new()
        .theme(BinaryColorTheme::OledWhite)
        .scale(1)
        .pixel_spacing(0) // Assuming macos scale is the same everywhere :)
        .build();
    let mut window = Window::new("My ST Home", &output_settings);

    // Create a new simulator display with 800x480 pixels.
    let mut display: SimulatorDisplay<BinaryColor> =
        SimulatorDisplay::new(Size::new(WIDTH + 100, HEIGHT));

    let mut frame = Frame::Calibration;

    'ui: loop {
        window.update(&display);

        // Slow down the loop to not consume 100% CPU
        std::thread::sleep(std::time::Duration::from_millis(1));

        // Consume the events
        for e in window.events() {
            debug!("Captured event {:?}", e);
            match e {
                SimulatorEvent::Quit => break 'ui,
                SimulatorEvent::KeyDown { keycode, .. } => {
                    let next_frame = match keycode {
                        Keycode::C => Some(Frame::Calibration),
                        Keycode::E => Some(Frame::Empty),
                        _ => None,
                    };

                    if let Some(next) = next_frame {
                        frame = next;
                    }

                    //display.clear(BinaryColor::Off).unwrap();
                    frame.draw(&mut display).unwrap();
                }
                _ => (),
            }
        }
    }
}
