#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    display::Display,
    get_key,
    keyboard::{KeyCode, KeyState},
    print,
};
use alloc::{format, string::String, vec, vec::Vec};
use core::panic::PanicInfo;
use embedded_graphics::{
    Drawable,
    geometry::{Dimensions, Point},
    mono_font::{MonoTextStyle, ascii::FONT_7X14, iso_8859_1::FONT_10X20},
    pixelcolor::Rgb565,
    prelude::{Primitive, RgbColor},
    primitives::{PrimitiveStyle, Rectangle},
    text::Text,
};
use embedded_layout::{
    View,
    align::{Align, horizontal, vertical},
    layout::linear::LinearLayout,
    prelude::Chain,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    print!("Starting Calculator app");
    let mut display = Display::take().unwrap();

    let mut input = vec!['e', 'x', 'p', 'r', ':', ' '];
    let input_min = input.len();
    let mut dirty = true;
    let mut last_area: Option<(Rectangle, Rectangle)> = None;

    LinearLayout::vertical(Chain::new(Text::new(
        "Calculator!",
        Point::zero(),
        MonoTextStyle::new(&FONT_10X20, Rgb565::WHITE),
    )))
    .arrange()
    .align_to(&display.bounding_box(), horizontal::Center, vertical::Top)
    .draw(&mut display)
    .expect("Failed to draw title");

    loop {
        if dirty {
            let style = PrimitiveStyle::with_fill(Rgb565::BLACK);
            if let Some(area) = last_area {
                Rectangle::new(area.0.top_left, area.0.size)
                    .into_styled(style)
                    .draw(&mut display)
                    .unwrap();

                Rectangle::new(area.1.top_left, area.1.size)
                    .into_styled(style)
                    .draw(&mut display)
                    .unwrap();
            }

            let text = input.iter().cloned().collect::<String>();

            let style = MonoTextStyle::new(&FONT_7X14, Rgb565::WHITE);
            let expr_layout = LinearLayout::vertical(Chain::new(Text::new(
                &text,
                display.bounding_box().center(),
                style,
            )))
            .arrange()
            .align_to(&display.bounding_box(), horizontal::Left, vertical::Center);

            let result = if let Ok(result) = evaluate(&input[input_min..]) {
                &format!(" = {}", result)
            } else {
                " = Error"
            };

            let eq_layout = LinearLayout::vertical(Chain::new(Text::new(
                result,
                display.bounding_box().center(),
                style,
            )))
            .arrange()
            .align_to(&display.bounding_box(), horizontal::Right, vertical::Center);

            last_area = Some((expr_layout.bounds(), eq_layout.bounds()));
            expr_layout.draw(&mut display).unwrap();
            eq_layout.draw(&mut display).unwrap();

            dirty = false;
        }

        let event = get_key();
        if event.state != KeyState::Idle {
            match event.key {
                KeyCode::Char(ch) => {
                    input.push(ch);
                }
                KeyCode::Del => {
                    input.truncate(input_min);
                }
                KeyCode::Backspace => {
                    if input.len() > input_min {
                        input.pop();
                    }
                }
                KeyCode::Esc => return,
                _ => (),
            }
            dirty = true;
        }
    }
}

fn get_int(int: &[char]) -> Result<i32, ()> {
    let mut output: i32 = 0;
    for &c in int {
        let digit = c.to_digit(10).ok_or(())? as i32;
        output = output
            .checked_mul(10)
            .and_then(|v| v.checked_add(digit))
            .ok_or(())?;
    }
    Ok(output)
}

fn primary(input: &[char], pos: &mut usize) -> Result<i32, ()> {
    let mut digits = Vec::new();
    while *pos < input.len() && input[*pos].is_ascii_digit() {
        digits.push(input[*pos]);
        *pos += 1;
    }
    if digits.is_empty() {
        return Err(());
    }
    get_int(&digits)
}

fn mul_div(input: &[char], pos: &mut usize) -> Result<i32, ()> {
    let mut value = primary(input, pos)?;
    while *pos < input.len() {
        let op = input[*pos];
        if op != '*' && op != '/' {
            break;
        }
        *pos += 1;
        let rhs = primary(input, pos)?;
        value = match op {
            '*' => value.checked_mul(rhs).ok_or(())?,
            '/' => {
                if rhs == 0 {
                    return Err(());
                }
                value.checked_div(rhs).ok_or(())?
            }
            _ => unreachable!(),
        };
    }
    Ok(value)
}

fn add_sub(input: &[char], pos: &mut usize) -> Result<i32, ()> {
    let mut value = mul_div(input, pos)?;
    while *pos < input.len() {
        let op = input[*pos];
        if op != '+' && op != '-' {
            break;
        }
        *pos += 1;
        let rhs = mul_div(input, pos)?;
        value = match op {
            '+' => value.checked_add(rhs).ok_or(())?,
            '-' => value.checked_sub(rhs).ok_or(())?,
            _ => unreachable!(),
        };
    }
    Ok(value)
}

fn evaluate(input: &[char]) -> Result<i32, ()> {
    let mut pos = 0;
    let result = add_sub(input, &mut pos)?;
    if pos != input.len() {
        return Err(());
    }
    Ok(result)
}
