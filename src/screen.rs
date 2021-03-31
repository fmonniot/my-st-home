use crate::delay::Delay;
use embedded_graphics::{
    fonts::{Font12x16, Font6x8, Text},
    prelude::*,
    primitives::{Circle, Line},
    style::PrimitiveStyle,
    text_style,
};
use embedded_hal::prelude::*;
use embedded_hal::{
    blocking::spi::Write,
    digital::v2::{InputPin, OutputPin},
};
use epd_waveshare::{
    color::*,
    epd7in5_v2::{Display7in5, EPD7in5},
    graphics::Display,
    prelude::*,
};

use tokio::task::JoinHandle;

pub struct ScreenHandle {
    sender: std::sync::mpsc::Sender<ScreenMessage>,
}

impl ScreenHandle {
    pub fn update(
        &self,
        msg: ScreenMessage,
    ) -> Result<(), std::sync::mpsc::SendError<ScreenMessage>> {
        self.sender.send(msg)
    }
}

pub enum ScreenMessage {
    UpdateLifxBulb { source: u32, power: bool },
}

// TODO Rename to something else, handle_message perhaps ?
fn screen_run_loop<SPI, SPIE, CS, BUSY, DC, RST>(
    receiver: std::sync::mpsc::Receiver<ScreenMessage>,
    mut screen: EPD7in5<SPI, CS, BUSY, DC, RST>,
    mut spi: SPI,
    mut delay: Delay,
) where
    SPI: Write<u8, Error = SPIE>,
    SPIE: std::fmt::Debug,
    CS: OutputPin,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
{
    // First a one off to test out things
    println!("Test all the rotations");
    let mut display = Display7in5::default(); // display is the display buffer

    // OK, that's going to be strange but Black is White, and White is Black…
    // Go figures why they got that one wrong XD
    display.clear_buffer(Color::Black);

    // draw a analog clock
    let _ = Circle::new(Point::new(64, 64), 64)
        .into_styled(PrimitiveStyle::with_stroke(White, 1))
        .draw(&mut display);
    let _ = Line::new(Point::new(64, 64), Point::new(0, 64))
        .into_styled(PrimitiveStyle::with_stroke(White, 1))
        .draw(&mut display);
    let _ = Line::new(Point::new(64, 64), Point::new(80, 80))
        .into_styled(PrimitiveStyle::with_stroke(White, 1))
        .draw(&mut display);

    let _ = Text::new("It's working-WoB!", Point::new(50, 200))
        .into_styled(text_style!(
            font = Font12x16,
            text_color = White,
            background_color = Black
        ))
        .draw(&mut display);
    draw_text(&mut display, "It's working-WoB!", 175, 250);

    screen.update_frame(&mut spi, &display.buffer()).unwrap();
    screen
        .display_frame(&mut spi)
        .expect("display frame new graphics");

    delay.delay_ms(5_000u16); // Wait for the screen to refresh, take between 2~5 seconds.

    println!("Clear buffer after tests");
    display.clear_buffer(Color::Black);
    screen.update_frame(&mut spi, &display.buffer()).unwrap();
    screen
        .display_frame(&mut spi)
        .expect("display frame new graphics");

    println!("Finished tests - going to sleep");
    screen.sleep(&mut spi).expect("screen goes to sleep");

    // Then the proper run loop
    for _message in receiver.recv() {}
}

#[cfg(target_os = "linux")]
fn create_run_loop(receiver: std::sync::mpsc::Receiver<ScreenMessage>) -> JoinHandle<()> {
    use rppal::gpio::Gpio;
    use rppal::spi::{Bus, Mode, SlaveSelect, Spi};

    println!("Initializing screen");

    // Configure SPI and GPIO
    let mut spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 4_000_000, Mode::Mode0).expect("spi bus");

    let gpio = Gpio::new().expect("gpio");
    let cs = gpio.get(8).expect("CS").into_output();
    let busy = gpio.get(24).expect("BUSY").into_input();
    let dc = gpio.get(25).expect("DC").into_output();
    let rst = gpio.get(17).expect("RST").into_output();

    let mut delay = Delay {};

    // Configure the screen before creating the run loop
    let epd7in5 =
        EPD7in5::new(&mut spi, cs, busy, dc, rst, &mut delay).expect("eink initalize error");

    // The screen run loop is entirely synchronous (SPI comm is sync and doesn't have an async
    // wrapper around it), so we use spawn_blocking
    tokio::task::spawn_blocking(move || {
        screen_run_loop(receiver, epd7in5, spi, delay);
    })
}

#[cfg(not(target_os = "linux"))]
fn create_run_loop(receiver: std::sync::mpsc::Receiver<ScreenMessage>) -> JoinHandle<()> {
    println!("No display available on this target. Screen messages will be ignored.");

    tokio::spawn(async move { for _message in receiver.recv() {} })
}

pub fn spawn() -> (JoinHandle<()>, ScreenHandle) {
    // Then create the communication channel
    let (sender, receiver) = std::sync::mpsc::channel();

    let handle = create_run_loop(receiver);

    (handle, ScreenHandle { sender })
}

fn draw_text(display: &mut Display7in5, text: &str, x: i32, y: i32) {
    let _ = Text::new(text, Point::new(x, y))
        .into_styled(text_style!(
            font = Font6x8,
            text_color = White,
            background_color = Black
        ))
        .draw(display);
}
