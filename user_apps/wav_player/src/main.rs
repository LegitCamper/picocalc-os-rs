#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use core::panic::PanicInfo;
mod wav_format;
use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::RgbColor,
};
use selection_ui::{SelectionUi, SelectionUiError, draw_text_center};
use userlib::{
    audio::{AUDIO_BUFFER_LEN, audio_buffer_ready, reconfigure_sample_rate, send_audio_buffer},
    display::Display,
    format,
    fs::{Entries, file_len, list_dir, read_file},
    get_key,
    keyboard::{KeyCode, KeyState},
    println,
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
    println!("Starting Wav player app");
    let Some(mut display) = Display::take() else {
        return;
    };

    loop {
        let mut entries = Entries::new();
        if let Err(error) = list_dir("/music", &mut entries) {
            println!("Cannot list /music: {:?}", error);
            return;
        }

        let mut files = entries.entries();
        files.retain(|entry| entry.extension().unwrap_or("").eq_ignore_ascii_case("wav"));
        let mut wavs = files.iter().map(|e| e.full_name()).collect::<Vec<&str>>();
        wavs.sort();

        let mut selection_ui = SelectionUi::new(&wavs);
        let selection = match selection_ui.run_selection_ui(&mut display) {
            Ok(maybe_sel) => maybe_sel,
            Err(e) => match e {
                SelectionUiError::SelectionListEmpty => {
                    draw_text_center(
                        &mut display,
                        "No Wavs were found in /music",
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

        draw_text_center(
            &mut display,
            &format!("Now playing {}", wavs[selection]),
            MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
        )
        .ok();

        let file_name = format!("/music/{}", wavs[selection]);
        let Ok(length) = file_len(&file_name) else {
            continue;
        };
        let mut read =
            |offset, buffer: &mut [u8]| read_file(&file_name, offset, buffer).map_err(|_| ());
        let Ok(wav) = wav_format::parse(length, &mut read) else {
            println!("Invalid WAV; only 8-bit stereo PCM is supported");
            continue;
        };
        if reconfigure_sample_rate(wav.sample_rate).is_err() {
            continue;
        }

        let mut buf = [127_u8; AUDIO_BUFFER_LEN];
        let mut position = wav.data_start;
        let mut remaining = wav.data_len;

        while remaining > 0 {
            if audio_buffer_ready() {
                buf.fill(127);
                let count = remaining.min(buf.len());
                if wav_format::read_exact_at(&mut read, position, &mut buf[..count]).is_err()
                    || send_audio_buffer(&buf).is_err()
                {
                    break;
                }
                position += count;
                remaining -= count;
            }

            let event = get_key();
            if event.state == KeyState::Released && event.key == KeyCode::Esc {
                return;
            }
        }
    }
}
