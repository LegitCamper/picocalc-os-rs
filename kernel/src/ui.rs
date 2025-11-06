use crate::{
    BINARY_CH,
    display::FRAMEBUFFER,
    elf::load_binary,
    framebuffer::FB_PAUSED,
    peripherals::keyboard,
    storage::{FileName, SDCARD},
    usb::{USB_ACTIVE, start_usb, stop_usb},
};
use abi_sys::keyboard::{KeyCode, KeyState};
use alloc::{str::FromStr, string::String, vec::Vec};
use core::sync::atomic::Ordering;
use embassy_futures::yield_now;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::Timer;
use embedded_graphics::{
    Drawable,
    mono_font::{
        MonoTextStyle,
        ascii::{FONT_4X6, FONT_10X20},
    },
    pixelcolor::Rgb565,
    prelude::{Dimensions, Point, Primitive, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Text},
};
use embedded_layout::{
    align::{horizontal, vertical},
    layout::linear::{FixedMargin, LinearLayout},
    prelude::*,
};
use embedded_text::TextBox;

pub async fn ui_handler() {
    let mut ui = UiState { page: UiPage::Menu };
    let mut menu = MenuPage {
        last_bounds: None,
        selection: 0,
        changed: true,
    };
    let mut scsi = ScsiPage { last_bounds: None };
    let mut overlay = Overlay::new();
    update_selections().await;

    loop {
        input_handler(&mut ui, &mut menu, &mut scsi).await;
        match ui.page {
            UiPage::Menu => menu.draw().await,
            UiPage::Scsi => scsi.draw().await,
        }
        overlay.draw().await;
        Timer::after_millis(5).await;
    }
}

async fn input_handler(ui: &mut UiState, menu: &mut MenuPage, scsi: &mut ScsiPage) {
    if let Some(event) = keyboard::read_keyboard_fifo().await {
        if event.state == KeyState::Pressed {
            match (&mut ui.page, event.key) {
                (UiPage::Menu, KeyCode::F1) => {
                    start_usb();
                    menu.clear().await;
                    ui.page = UiPage::Scsi;
                }
                (UiPage::Scsi, KeyCode::F1) => {
                    stop_usb();
                    scsi.clear().await;
                    ui.page = UiPage::Menu;
                    update_selections().await;
                }
                (UiPage::Menu, _) => menu.handle_input(event.key).await,
                (UiPage::Scsi, _) => scsi.handle_input(event.key).await,
            }
        }
    }
}

struct Overlay {
    f1_label: &'static str,
    last_bounds: Option<Rectangle>,
}

impl Overlay {
    pub fn new() -> Self {
        Self {
            f1_label: "Press F1 to enable/disable mass storage",
            last_bounds: None,
        }
    }

    async fn draw(&mut self) {
        let text_style = MonoTextStyle::new(&FONT_4X6, Rgb565::WHITE);
        let fb = unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() };
        let bounds = fb.bounding_box();

        let text = Text::with_alignment(
            self.f1_label,
            Point::new(10, bounds.size.height as i32 - 24), // bottom-left corner
            text_style,
            Alignment::Left,
        );

        self.last_bounds = Some(text.bounds());
        text.draw(fb).unwrap();
    }
}

enum UiPage {
    Menu,
    Scsi,
}

struct UiState {
    page: UiPage,
}

trait Page {
    async fn draw(&mut self);
    async fn handle_input(&mut self, key: KeyCode);
    async fn clear(&mut self);
}

struct ScsiPage {
    last_bounds: Option<Rectangle>,
}

impl Page for ScsiPage {
    async fn draw(&mut self) {
        let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
        let fb = unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() };
        let bounds = fb.bounding_box();

        Text::with_alignment(
            "Mass storage over usb enabled",
            bounds.center(),
            text_style,
            Alignment::Center,
        )
        .draw(fb)
        .unwrap();
    }

    async fn handle_input(&mut self, _key: KeyCode) {
        ()
    }

    async fn clear(&mut self) {
        if let Some(rect) = self.last_bounds {
            clear_rect(rect).await;
        }
    }
}

static SELECTIONS: Mutex<CriticalSectionRawMutex, Vec<FileName>> = Mutex::new(Vec::new());
static mut SELECTIONS_CHANGED: bool = true;

async fn update_selections() {
    while USB_ACTIVE.load(Ordering::Acquire) {
        Timer::after_millis(50).await;
    }
    let mut guard = SDCARD.get().lock().await;
    let sd = guard.as_mut().unwrap();

    let files = sd.list_files_by_extension(".bin").unwrap();
    let mut selections = SELECTIONS.lock().await;

    if *selections != files {
        *selections = files;
        unsafe { SELECTIONS_CHANGED = true }
    }
}
struct MenuPage {
    last_bounds: Option<Rectangle>,
    selection: usize,
    changed: bool,
}

impl Page for MenuPage {
    async fn draw(&mut self) {
        if self.changed {
            self.clear().await;
        }

        let guard = SELECTIONS.lock().await;
        let file_names = &guard.clone();

        let text_style = MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE);
        let display_area = unsafe { FRAMEBUFFER.as_mut().unwrap().bounding_box() };

        const NO_BINS: &str = "No Programs found on SD Card. Ensure programs end with '.bin', and are located in the root directory";
        let no_bins = String::from_str(NO_BINS).unwrap();

        FB_PAUSED.store(true, Ordering::Release); // ensure all elements show up at once

        if file_names.is_empty() {
            TextBox::new(
                &no_bins,
                Rectangle::new(
                    Point::new(25, 25),
                    Size::new(display_area.size.width - 50, display_area.size.width - 50),
                ),
                text_style,
            )
            .draw(unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() })
            .unwrap();
        } else {
            let mut views: alloc::vec::Vec<Text<MonoTextStyle<Rgb565>>> = Vec::new();

            for i in file_names {
                views.push(Text::new(&i.long_name, Point::zero(), text_style));
            }

            let views_group = Views::new(views.as_mut_slice());

            let layout = LinearLayout::vertical(views_group)
                .with_alignment(horizontal::Center)
                .with_spacing(FixedMargin(5))
                .arrange()
                .align_to(&display_area, horizontal::Center, vertical::Center);

            // draw selected box
            let selected_bounds = layout
                .inner()
                .get(self.selection)
                .expect("Selected binary missing")
                .bounding_box();
            Rectangle::new(selected_bounds.top_left, selected_bounds.size)
                .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
                .draw(unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() })
                .unwrap();

            self.last_bounds = Some(layout.bounds());

            layout
                .draw(unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() })
                .unwrap();
        }

        self.changed = false;
        FB_PAUSED.store(false, Ordering::Release); // ensure all elements show up at once
    }

    async fn handle_input(&mut self, key: KeyCode) {
        match key {
            KeyCode::Enter | KeyCode::Right => {
                let selections = SELECTIONS.lock().await;
                let selection = selections[self.selection].clone();

                BINARY_CH.send(selection.short_name).await;
                self.clear().await;
                loop {
                    yield_now().await;
                }
            }
            KeyCode::Up => {
                if self.selection > 0 {
                    self.selection -= 1;
                } else {
                    let len = SELECTIONS.lock().await.len();
                    if len > 0 {
                        self.selection = len - 1;
                    }
                }
            }
            KeyCode::Down => {
                if self.selection + 1 < SELECTIONS.lock().await.len() {
                    self.selection += 1;
                } else {
                    self.selection = 0; // wrap to top
                }
            }
            _ => (),
        }
        self.changed = true;
    }

    async fn clear(&mut self) {
        if let Some(rect) = self.last_bounds {
            clear_rect(rect).await;
        }
    }
}

pub async fn clear_screen() {
    clear_rect(Rectangle {
        top_left: Point::zero(),
        size: Size::new(320, 320),
    })
    .await
}

async fn clear_rect(rect: Rectangle) {
    let fb = unsafe { &mut *FRAMEBUFFER.as_mut().unwrap() };
    Rectangle::new(rect.top_left, rect.size)
        .into_styled(PrimitiveStyle::with_fill(Rgb565::BLACK))
        .draw(fb)
        .unwrap();
}
