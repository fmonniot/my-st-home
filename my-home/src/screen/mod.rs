use embedded_graphics::{
    fonts::{Font6x8, Text},
    pixelcolor::BinaryColor,
    primitives::{Circle, Line, Rectangle},
    style::PrimitiveStyle,
    style::TextStyleBuilder,
    text_style, DrawTarget,
};
use embedded_layout::{
    layout::linear::{spacing::FixedMargin, LinearLayout},
    prelude::*,
};
use epd_waveshare::color::*;

mod actor;
mod delay;
mod screen;

pub use actor::{new, UserInterface};

use screen::Screen;

#[derive(Debug, Clone)]
pub enum ScreenMessage {
    UpdateLifxBulb { source: u32, power: bool },
    UpdateLifxGroup { group: String, brightness: u16 },
}

impl crate::actor::Message for ScreenMessage {}

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
#[derive(Debug, Clone)]
pub enum Frame {
    Calibration, // To test out the implementation, needs to be changed to something meaningful :)
    Empty,
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
            (frame, _) => frame,
        }
    }

    /// Draw the current state onto a buffer. The buffer isn't cleared.
    pub fn draw<D: DrawTarget<BinaryColor>>(&self, display: &mut D) -> Result<(), D::Error> {
        match &self {
            Frame::Calibration => draw_calibration(display),
            Frame::Empty => {
                display.clear(BinaryColor::Off)?;

                Ok(())
            }
        }
    }
}

fn draw_calibration<D: DrawTarget<BinaryColor>>(display: &mut D) -> Result<(), D::Error> {
    // Debug information
    // draw a rectangle around the screen
    Rectangle::new(Point::new(1, 1), Point::new(WIDTH - 1, HEIGHT - 1))
        .into_styled(PrimitiveStyle::with_stroke(White, 1))
        .draw(display)?;

    // Text in the four corners
    draw_text(display, "top-left", 1, 1);
    draw_text(display, "top-right", WIDTH - 6 * 9 - 1, 1);
    draw_text(display, "bottom-left", 1, HEIGHT - 8 - 1);
    draw_text(display, "bottom-right", WIDTH - 6 * 12 - 1, HEIGHT - 8 - 1);

    // Draw the frame (Fixed frame for now, will go into a pattern match to choose what to display)

    let display_area = display.display_area();

    let mut clock = AnalogClock::new(Size::new(128, 128));
    clock.translate(Point::new(10, 10));

    let calendar = CalendarEventWidget::new("New event", "Thu 08 Apr", Size::new(200, 40));

    LinearLayout::horizontal()
        .with_spacing(FixedMargin(4))
        .add_view(clock)
        .add_view(calendar)
        .arrange()
        .align_to(&display_area, horizontal::Center, vertical::Center)
        .draw(display)?;

    Ok(())
}

struct CalendarEventWidget {
    title: String,
    date: String,
    bounds: Rectangle,
}

impl CalendarEventWidget {
    fn new(title: &str, date: &str, size: Size) -> CalendarEventWidget {
        CalendarEventWidget {
            title: title.to_string(),
            date: date.to_string(),
            bounds: Rectangle::with_size(Point::zero(), size),
        }
    }
}

impl View for CalendarEventWidget {
    #[inline]
    fn translate(&mut self, by: Point) {
        self.bounds.translate(by);
    }

    #[inline]
    fn bounds(&self) -> Rectangle {
        self.bounds
    }
}

impl Drawable<BinaryColor> for &CalendarEventWidget {
    fn draw<D: DrawTarget<BinaryColor>>(self, display: &mut D) -> Result<(), D::Error> {
        // Create styles
        let border_style = PrimitiveStyle::with_stroke(White, 1);
        let text_style = TextStyleBuilder::new(Font6x8)
            .text_color(White)
            .background_color(Black)
            .build();

        // Create a 1px border
        let border = self.bounds.into_styled(border_style);

        // Create the title and dates
        let mut information = LinearLayout::vertical()
            .with_alignment(horizontal::Center)
            .add_view(Text::new(&self.title, Point::new(2, 0)).into_styled(text_style))
            .add_view(Text::new(&self.date, Point::new(4, 0)).into_styled(text_style))
            .arrange();

        // Align the text within the border, with some margin on the left
        information
            .align_to_mut(&border, horizontal::Left, vertical::Center)
            .translate(Point::new(2, 0));

        // Draw everything
        border.draw(display)?;
        information.draw(display)?;

        Ok(())
    }
}

struct AnalogClock {
    bounds: Rectangle,
}

impl AnalogClock {
    fn new(size: Size) -> AnalogClock {
        AnalogClock {
            bounds: Rectangle::with_size(Point::zero(), size),
        }
    }
}

impl View for AnalogClock {
    #[inline]
    fn translate(&mut self, by: Point) {
        self.bounds.translate(by);
    }

    #[inline]
    fn bounds(&self) -> Rectangle {
        self.bounds
    }
}

impl Drawable<BinaryColor> for &AnalogClock {
    fn draw<D: DrawTarget<BinaryColor>>(self, display: &mut D) -> Result<(), D::Error> {
        println!(
            "drawing analog clock within {:?}. bounds = {:?}",
            display.display_area(),
            self.bounds
        );

        let center = self.bounds.center();

        // TODO Find the size of the circle by looking at the min distance from center to border.

        // TODO Do a real calculus for those two :)
        let long = Point::new(self.bounds.top_left.x + 2, center.y);
        let short = Point::new(center.x + 18, center.y + 16);

        // TODO Do we need to find the center point in the bounds ?
        Circle::new(center, 64)
            .into_styled(PrimitiveStyle::with_stroke(White, 1))
            .draw(display)?;

        Line::new(center, long)
            .into_styled(PrimitiveStyle::with_stroke(White, 1))
            .draw(display)?;

        Line::new(center, short)
            .into_styled(PrimitiveStyle::with_stroke(White, 1))
            .draw(display)?;

        Ok(())
    }
}
