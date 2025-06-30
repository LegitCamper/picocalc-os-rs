pub mod screen {
    use arrform::{ArrForm, arrform};
    use embedded_graphics::{
        Drawable,
        draw_target::DrawTarget,
        mono_font::{MonoTextStyle, ascii::FONT_10X20},
        pixelcolor::Rgb565,
        prelude::{Point, RgbColor, Size},
        primitives::Rectangle,
        text::Text,
    };
    use embedded_layout::{
        align::{horizontal, vertical},
        layout::linear::LinearLayout,
        prelude::*,
    };

    pub const SCREEN_WIDTH: usize = 320;
    pub const SCREEN_HEIGHT: usize = 320;

    pub const STATUS_BAR_WIDTH: usize = 320;
    pub const STATUS_BAR_HEIGHT: usize = 40;

    pub struct UI {
        pub status_bar: StatusBar,
    }

    impl UI {
        pub fn new() -> Self {
            Self {
                status_bar: StatusBar {
                    battery: 100,
                    backlight: 100,
                    volume: 100,
                },
            }
        }

        pub fn draw_status_bar<D: DrawTarget<Color = Rgb565>>(&mut self, target: &mut D) {
            let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);

            let status_bar = Rectangle::new(
                Point::new(0, 0),
                Size::new(STATUS_BAR_WIDTH as u32, STATUS_BAR_HEIGHT as u32),
            );
            let _ = LinearLayout::horizontal(
                Chain::new(Text::new(
                    arrform!(20, "Bat: {}", self.status_bar.battery).as_str(),
                    Point::zero(),
                    text_style,
                ))
                .append(Text::new(
                    arrform!(20, "Lght: {}", self.status_bar.backlight).as_str(),
                    Point::zero(),
                    text_style,
                ))
                .append(Text::new(
                    arrform!(20, "Vol: {}", self.status_bar.volume).as_str(),
                    Point::zero(),
                    text_style,
                )),
            )
            .arrange()
            .align_to(&status_bar, horizontal::Center, vertical::Center)
            .draw(target);
        }
    }

    pub struct StatusBar {
        pub battery: u8,
        pub backlight: u8,
        pub volume: u8,
    }
}
