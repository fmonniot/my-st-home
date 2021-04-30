use super::{screen, Frame, Screen, ScreenMessage};
use crate::actor::{Actor, Context, Message, Receiver};
use epd_waveshare::{color::*, epd7in5_v2::Display7in5, graphics::Display};
use log::info;

// UserInterface is a bit misleading, as it's only outputting information
// to the user. Another actor is responsible to get the user input, and thus
// could also be called UI.
pub struct UserInterface {
    screen: Screen,
    // TODO Once we have the simulator in place, see if we need a special struct
    // to implement the Display trait. If not, we should have our own and not depends
    // on epd_waveshare on mac at all. Maybe ? Not sure about that one tbh.
    // Adding to the comment above, if I don't need rotation I think it's fine to switch
    // to DrawTarget<BinaryColor>. We'd have to re-implement clearing the buffer, but that
    // seems easy enough to do (there might be tools in embedded-graphics already).
    // That being said, we still have to cross thread for mac so not sure if we gain much by removing
    // the concrete implementation here. Not that important as we only care about performance
    // on linux/raspberry.
    display: Display7in5,

    // The last displayed frame. If none, nothing was displayed (blank screen).
    current_frame: Option<Frame>,
}

#[derive(Debug, Clone)]
struct DrawFrame(Frame);

impl Message for DrawFrame {}

pub fn new() -> UserInterface {
    let screen: Screen = screen::create().expect("screen initialized");

    UserInterface {
        screen,
        display: Display7in5::default(),
        current_frame: None,
    }
}

impl Actor for UserInterface {
    fn pre_start(&mut self, ctx: &Context<Self>) {
        self.current_frame = Some(Frame::Calibration);
        ctx.myself.send_msg(DrawFrame(Frame::Calibration));
    }

    fn post_stop(&mut self, _ctx: &Context<Self>) {
        info!("UserInterface has shut down")
    }
}

impl Message for ScreenMessage {}

impl Receiver<ScreenMessage> for UserInterface {
    fn recv(&mut self, ctx: &Context<Self>, msg: ScreenMessage) {
        let frame = std::mem::take(&mut self.current_frame);
        let frame = frame.unwrap_or_else(|| Frame::Calibration);

        let next = frame.update(msg);
        self.current_frame = Some(next.clone());

        ctx.myself.send_msg(DrawFrame(next));
    }
}

impl Receiver<DrawFrame> for UserInterface {
    // Note that this function will actually block the thread while interacting
    // with self.screen. That's because of how the SPI implementation is made.
    fn recv(&mut self, _ctx: &Context<Self>, msg: DrawFrame) {
        let new_frame = msg.0;

        // OK, that's going to be strange but Black is White, and White is Black…
        // Go figures why they got that one wrong XD.
        // BUT, it's not the case when working with the simulator…
        // Not sure how to solve this, maybe inversing the buffer before transmitting
        // to the e-ink device ? Or opening an issue with the lib and see what they have to say.
        // Future me note: Actually it kind of make sense, except we probably
        // shouldn't use the color names. White is actually BinaryColor::Off. LCD and e-ink
        // have inversed pixel luminosity: LCD needs current to shine whereas e-ink needs
        // current to move the black molecule to the surface.
        // TODO Aliases which change depending on the compilation target ?
        self.display.clear_buffer(Color::Black);

        // update the buffer
        new_frame.draw(&mut self.display).expect("paint frame");

        // update the screen
        self.screen.wake_up().unwrap(); // TODO Error handling
        self.screen
            .update_and_display_frame(&self.display.buffer())
            .expect("display frame new graphics");
        self.screen.sleep().expect("screen goes to sleep");
    }
}
