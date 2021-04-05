use crate::delay::Delay;
use embedded_graphics::{
    fonts::{Font12x16, Font6x8, Text},
    prelude::*,
    primitives::{Circle, Line},
    style::PrimitiveStyle,
    text_style,
};
use embedded_hal::prelude::*;
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
        let mut delay = Delay {};

        // TODO Once we have the simulator in place, see if we need a special struct
        // to implement the Display trait. If not, we should have our own and not depends
        // on epd_waveshare on mac at all. Maybe ? Not sure about that one tbh.
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

        screen
            .update_and_display_frame(&display.buffer())
            .expect("display frame new graphics");

        delay.delay_ms(5_000u16); // Wait for the screen to refresh, take between 2~5 seconds.

        println!("Clear buffer after tests");
        display.clear_buffer(Color::Black);

        screen
            .update_and_display_frame(&display.buffer())
            .expect("display frame new graphics");

        println!("Finished tests - going to sleep");
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
                    debug!("new screen message: {:?}", message);
                    match message {
                        ScreenMessage::UpdateLifxBulb { .. } => {
                            display.clear_buffer(Color::Black);

                            draw_text(&mut display, &format!("Received: {:?}", message), 175, 250);
                            screen
                                .update_and_display_frame(&display.buffer())
                                .expect("display frame new graphics");
                        }

                        ScreenMessage::UpdateLifxGroup { .. } => {
                            display.clear_buffer(Color::Black);

                            draw_text(&mut display, &format!("Received: {:?}", message), 175, 250);
                            screen
                                .update_and_display_frame(&display.buffer())
                                .expect("display frame new graphics");
                        }
                    }
                }
            }
        }

        warn!("Ending the screen actor");
    });

    let task = ScreenTask { sender, join };

    (task, runloop)
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
