#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    Rng,
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    get_key,
    keyboard::{KeyCode, KeyState},
    print, sleep,
};
use core::panic::PanicInfo;
use embedded_graphics::{pixelcolor::Rgb565, prelude::RgbColor};
use embedded_snake::{Direction, SnakeGame};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

const CELL_SIZE: usize = 8;

pub fn main() {
    print!("Starting Snake app");
    let mut display = Display::take().unwrap();

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

    loop {
        let event = get_key();
        if event.state != KeyState::Idle {
            let direction = match event.key {
                KeyCode::Up => Direction::Up,
                KeyCode::Down => Direction::Down,
                KeyCode::Right => Direction::Right,
                KeyCode::Left => Direction::Left,
                KeyCode::Esc => return,
                _ => Direction::None,
            };
            game.set_direction(direction);
        };

        // ensure all draws show up at once
        game.pre_draw(&mut display);
        game.draw(&mut display);

        sleep(15);
    }
}
