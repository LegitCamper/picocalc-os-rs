use embedded_graphics::{pixelcolor::Rgb565, prelude::Size};
use embedded_graphics_simulator::{
    BinaryColorTheme, OutputSettingsBuilder, SimulatorDisplay, Window,
};

use shared::screen::{SCREEN_HEIGHT, SCREEN_WIDTH, UI};

fn main() -> Result<(), core::convert::Infallible> {
    let mut display =
        SimulatorDisplay::<Rgb565>::new(Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32));

    let mut ui = UI::new();

    ui.draw_status_bar(&mut display);

    let output_settings = OutputSettingsBuilder::new().build();
    Window::new("Hello World", &output_settings).show_static(&display);

    Ok(())
}
