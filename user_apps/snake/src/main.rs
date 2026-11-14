#![no_std]
#![no_main]

extern crate alloc;
mod timing;
use core::panic::PanicInfo;
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
};
use embedded_snake::{Direction, SnakeGame};
use userlib::{
    Rng,
    display::{Display, FrameBuffer, SCREEN_HEIGHT, SCREEN_WIDTH},
    get_key, get_ms,
    keyboard::{KeyCode, KeyState},
    println, sleep,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {
        userlib::sleep(1000);
    }
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

const CELL_SIZE: usize = 8;

pub fn main() {
    println!("Starting Snake app");
    let Some(mut display) = Display::take() else {
        return;
    };
    let Ok(mut canvas) = FrameBuffer::new(Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32))
    else {
        println!("Not enough memory for Snake canvas");
        return;
    };

    let mut game = SnakeGame::<100, Rgb565, Rng>::new(
        SCREEN_WIDTH as u16,
        SCREEN_HEIGHT as u16,
        CELL_SIZE as u16,
        CELL_SIZE as u16,
        Rng,
        Rgb565::BLACK,
        Rgb565::GREEN,
        Rgb565::RED,
        50,
    );

    let mut last_step = get_ms();
    loop {
        let event = get_key();
        if event.state == KeyState::Pressed {
            let direction = match event.key {
                KeyCode::Up => Some(Direction::Up),
                KeyCode::Down => Some(Direction::Down),
                KeyCode::Right => Some(Direction::Right),
                KeyCode::Left => Some(Direction::Left),
                KeyCode::Esc => return,
                _ => None,
            };
            if let Some(direction) = direction {
                game.set_direction(direction);
            }
        };

        let now = get_ms();
        let remaining = timing::remaining_ms(now, last_step);
        if remaining > 0 {
            sleep(remaining.min(10));
            continue;
        }
        last_step = now;
        game.pre_draw(&mut canvas);
        game.draw(&mut canvas);
        if canvas.present(&mut display, Point::zero()).is_err() {
            return;
        }
    }
}
