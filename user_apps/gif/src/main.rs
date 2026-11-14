#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use core::panic::PanicInfo;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::{DrawTarget, Point, RgbColor, Size},
};
use selection_ui::{SelectionUi, SelectionUiError, draw_text_center};
use tinygif::Gif;
use userlib::{
    FramePacer,
    display::{Display, FrameBuffer, SCREEN_HEIGHT, SCREEN_WIDTH},
    fs::{file_len, list_media, read_file},
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

pub fn main() {
    println!("Starting Gif app");
    let Some(mut display) = Display::take() else {
        return;
    };

    let files = match list_media("/gifs", "gif") {
        Ok(files) => files,
        Err(error) => {
            println!("Cannot list GIFs: {:?}", error);
            return;
        }
    };
    let gifs = files.iter().map(|path| path.as_str()).collect::<Vec<_>>();

    let mut selection_ui = SelectionUi::new(&gifs);
    let selection = match selection_ui.run_selection_ui(&mut display) {
        Ok(maybe_sel) => maybe_sel,
        Err(e) => match e {
            SelectionUiError::SelectionListEmpty => {
                draw_text_center(
                    &mut display,
                    "No GIFs in / or /gifs",
                    MonoTextStyle::new(&FONT_6X10, Rgb565::RED),
                )
                .ok();
                None
            }
            SelectionUiError::DisplayError(_) => None,
        },
    };

    let Some(selection) = selection else {
        return;
    };

    let file_name = gifs[selection];
    let Ok(size) = file_len(file_name) else {
        return;
    };
    // ponytail: whole-file decoding is capped at 2 MiB; stream larger GIFs later.
    if size == 0 || size > 2_097_152 {
        println!("GIF is empty or exceeds 2 MiB");
        return;
    }
    let mut buf = Vec::new();
    if buf.try_reserve_exact(size).is_err() {
        println!("Not enough memory for GIF");
        return;
    }
    buf.resize(size, 0);
    let mut position = 0;
    while position < size {
        match read_file(file_name, position, &mut buf[position..]) {
            Ok(0) | Err(_) => return,
            Ok(read) => position += read,
        }
    }

    let Ok(gif) = Gif::<Rgb565>::from_slice(&buf) else {
        println!("Invalid GIF");
        return;
    };
    // ponytail: canvas is capped at the display size; add clipping if larger GIFs matter.
    if usize::from(gif.width()) > SCREEN_WIDTH || usize::from(gif.height()) > SCREEN_HEIGHT {
        println!("GIF exceeds display dimensions");
        return;
    }

    let translation = Point::new(
        (SCREEN_WIDTH as i32 - gif.width() as i32) / 2,
        (SCREEN_HEIGHT as i32 - gif.height() as i32) / 2,
    );

    let Ok(mut canvas) =
        FrameBuffer::new(Size::new(u32::from(gif.width()), u32::from(gif.height())))
    else {
        println!("Not enough memory for GIF canvas");
        return;
    };
    let _ = display.clear(Rgb565::BLACK);
    let mut pacer = FramePacer::new(get_ms());
    let mut report_at = get_ms();
    let mut presented = 0_u32;
    let mut skipped = 0_u32;
    let mut decode_ms = 0_u64;
    let mut last_decode_ms = 0_u64;
    let mut write_ms = 0_u64;

    loop {
        let mut has_frames = false;
        let mut frames = gif.frames().peekable();
        while let Some(frame) = frames.next() {
            has_frames = true;
            if exit_requested() {
                return;
            }
            // ponytail: zero-delay GIFs use 10 ms; explicit positive GIF delays are preserved.
            pacer.advance(u64::from(frame.delay_centis.max(1)) * 10);
            if !pacer.should_present(get_ms().saturating_add(last_decode_ms))
                && frames.peek().is_some_and(|next| next.is_self_contained())
            {
                skipped = skipped.saturating_add(1);
                continue;
            }
            let start = get_ms();
            if frame.draw_checked(&mut canvas).is_err() {
                println!("Invalid GIF frame");
                return;
            }
            last_decode_ms = get_ms().saturating_sub(start);
            decode_ms = decode_ms.saturating_add(last_decode_ms);
            if pacer.should_present(get_ms()) {
                let start = get_ms();
                if canvas.present(&mut display, translation).is_err() {
                    return;
                }
                let finished = get_ms();
                write_ms = write_ms.saturating_add(finished.saturating_sub(start));
                pacer.presented(finished);
                presented = presented.saturating_add(1);
            } else {
                skipped = skipped.saturating_add(1);
            }

            let now = get_ms();
            if now.saturating_sub(report_at) >= 5_000 {
                println!(
                    "GIF {} ms: presented={}, skipped={}, decode_ms={}, write_ms={}",
                    now.saturating_sub(report_at),
                    presented,
                    skipped,
                    decode_ms,
                    write_ms
                );
                report_at = now;
                presented = 0;
                skipped = 0;
                decode_ms = 0;
                write_ms = 0;
            }

            loop {
                if exit_requested() {
                    return;
                }
                let remaining = pacer.remaining_ms(get_ms());
                if remaining == 0 {
                    break;
                }
                sleep(remaining.min(10));
            }
        }
        if !has_frames {
            return;
        }
    }
}

fn exit_requested() -> bool {
    let event = get_key();
    event.state != KeyState::Idle && event.key == KeyCode::Esc
}
