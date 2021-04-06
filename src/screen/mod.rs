use crate::delay::Delay;
use embedded_graphics::{
    fonts::{Font6x8, Text},
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, Rectangle},
    style::PrimitiveStyle,
    text_style,
};
use epd_waveshare::{color::*, epd7in5_v2::Display7in5, graphics::Display};

use log::{debug, warn};
use tokio::task::JoinHandle;
mod screen;

use screen::Screen;

pub struct ScreenTask {
    sender: std::sync::mpsc::Sender<ScreenMessage>,
    join: JoinHandle<()>,
}

impl ScreenTask {
    pub fn handle(&self) -> ScreenHandle {
        ScreenHandle {
            sender: self.sender.clone(),
        }
    }

    pub async fn join(self) -> Result<(), tokio::task::JoinError> {
        self.join.await?;
        Ok(())
    }
}

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

    pub fn update_group_brightness(
        &self,
        group: &str,
        brightness: u16,
    ) -> Result<(), std::sync::mpsc::SendError<ScreenMessage>> {
        self.sender.send(ScreenMessage::UpdateLifxGroup {
            group: group.into(),
            brightness,
        })
    }
}

#[derive(Debug)]
pub enum ScreenMessage {
    UpdateLifxBulb { source: u32, power: bool },
    UpdateLifxGroup { group: String, brightness: u16 },
}

pub fn spawn() -> (ScreenTask, impl FnOnce() -> ()) {
    // Then create the communication channel
    let (sender, receiver) = std::sync::mpsc::channel();
    let (mut screen, runloop) = screen::create().expect("screen initialized");

    let join = tokio::task::spawn_blocking(move || {
        // Is it still needed ? Do we want to limit the number of refresh we can do
        // per 5 seconds period ?
        let mut delay = Delay {};

        // TODO Once we have the simulator in place, see if we need a special struct
        // to implement the Display trait. If not, we should have our own and not depends
        // on epd_waveshare on mac at all. Maybe ? Not sure about that one tbh.
        // Adding to the comment above, if I don't need rotation I think it's fine to switch
        // to DrawTarget<BinaryColor>. We'd have to re-implement clearing the buffer, but that
        // seems easy enough to do (there might be tools in embedded-graphics already).
        // That being said, we still have to cross thread for mac so not sure if we gain much by removing 
        // the concrete implementation here. Not that important as we only care about performance
        // on linux/raspberry.
        let mut display = Display7in5::default(); // display is the display buffer

        // OK, that's going to be strange but Black is White, and White is Black…
        // Go figures why they got that one wrong XD.
        // BUT, it's not the case when working with the simulator…
        // Not sure how to solve this, maybe inversing the buffer before transmitting
        // to the e-ink device ? Or opening an issue with the lib and see what they have to say.
        display.clear_buffer(Color::Black);

        let mut frame = Frame::Calibration;

        frame.draw(&mut display).expect("paint frame");
        screen
            .update_and_display_frame(&display.buffer())
            .expect("display frame new graphics");
        screen.sleep().expect("screen goes to sleep");

        loop {
            // Blocking until we receive the next message (or the channel closed)
            // We don't need to use an async channel as `screen` calls block when using the e-ink device.
            match receiver.recv() {
                Err(_) => {
                    // All Sender have closed, ending the loop
                    break;
                }
                Ok(message) => {
                    debug!("Received screen message: {:?}", message);

                    // state update
                    frame = frame.update(message);

                    // update the buffer
                    display.clear_buffer(Color::Black);
                    frame.draw(&mut display).expect("paint frame");

                    // update the screen
                    screen.wake_up(&mut delay).unwrap(); // TODO Error handling
                    screen
                        .update_and_display_frame(&display.buffer())
                        .expect("display frame new graphics");
                    screen.sleep().expect("screen goes to sleep");
                }
            }
        }

        warn!("Ending the screen actor");
    });

    let task = ScreenTask { sender, join };

    (task, runloop)
}

// TODO Return error
fn draw_text<D: DrawTarget<BinaryColor>>(display: &mut D, text: &str, x: i32, y: i32) {
    let _ = Text::new(text, Point::new(x, y))
        .into_styled(text_style!(
            font = Font6x8,
            text_color = White,
            background_color = Black
        ))
        .draw(display);
}


/// Frame is the representation of what is on screen.
/// It is also a state machine and define what the next frame will be based on a [ScreenMessage]
enum Frame {
    Calibration, // To test out the implementation, needs to be changed to something meaningful :)
}

/// Width of the display
pub const WIDTH: i32 = 800;
/// Height of the display
pub const HEIGHT: i32 = 480;

impl Frame {

    /// State machine to update the current state with an update message
    fn update(self, message: ScreenMessage) -> Frame {

        match (self, message) {
            (Frame::Calibration, _) => Frame::Calibration,
        }
    } 

    /// Draw the current state onto a buffer. The buffer isn't cleared.
    fn draw<D: DrawTarget<BinaryColor>>(&self, display: &mut D) -> Result<(), D::Error> {
        // draw a rectangle around the screen
        Rectangle::new(Point::new(1, 1), Point::new(WIDTH - 1, HEIGHT - 1))
            .into_styled(PrimitiveStyle::with_stroke(White, 1))
            .draw(display)?;

        // Text in the four corners
        draw_text(display, "top-left", 1, 1);
        draw_text(display, "top-right", WIDTH - 6 * 9 - 1, 1);
        draw_text(display, "bottom-left", 1, HEIGHT - 8 - 1);
        draw_text(display, "bottom-right", WIDTH - 6 * 12 - 1, HEIGHT - 8 - 1);

        // draw a analog clock
        Circle::new(Point::new(66, 64), 64)
            .into_styled(PrimitiveStyle::with_stroke(White, 1))
            .draw(display)?;

        Line::new(Point::new(66, 64), Point::new(2, 64))
            .into_styled(PrimitiveStyle::with_stroke(White, 1))
            .draw(display)?;

        Line::new(Point::new(66, 64), Point::new(82, 80))
            .into_styled(PrimitiveStyle::with_stroke(White, 1))
            .draw(display)?;

        // And some text in the middle of the screen
        draw_text(
            display,
            "It's working-WoB!",
            WIDTH /2 - (17 / 2),
            HEIGHT / 2 - 4,
        );

        Ok(())
    }
}
