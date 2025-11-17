#![no_std]
#![no_main]

extern crate alloc;
use alloc::{string::String, vec::Vec};
use core::panic::PanicInfo;
use embedded_audio::{wav::Wav, AudioFile, PlatformFile, PlatformFileError};
use embedded_graphics::{
    mono_font::{ascii::FONT_6X10, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::RgbColor,
};
use selection_ui::{draw_text_center, SelectionUi, SelectionUiError};
use userlib::{
    audio::{audio_buffer_ready, send_audio_buffer, AUDIO_BUFFER_LEN},
    display::Display,
    format,
    fs::{file_len, list_dir, read_file, Entries},
    get_key,
    keyboard::{KeyCode, KeyState},
    println,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("user panic: {} @ {:?}", info.message(), info.location(),);
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    println!("Starting Wav player app");
    let mut display = Display::take().unwrap();

    loop {
        let mut entries = Entries::new();
        list_dir("/music", &mut entries);

        let mut files = entries.entries();
        files.retain(|e| e.extension().unwrap_or("") == "wav");
        let mut wavs = files.iter().map(|e| e.full_name()).collect::<Vec<&str>>();
        wavs.sort();

        let mut selection_ui = SelectionUi::new(&mut wavs);
        let selection = match selection_ui.run_selection_ui(&mut display) {
            Ok(maybe_sel) => maybe_sel,
            Err(e) => match e {
                SelectionUiError::SelectionListEmpty => {
                    draw_text_center(
                        &mut display,
                        "No Wavs were found in /music",
                        MonoTextStyle::new(&FONT_6X10, Rgb565::RED),
                    )
                    .expect("Display Error");
                    None
                }
                SelectionUiError::DisplayError(_) => panic!("Display Error"),
            },
        };

        assert!(selection.is_some());

        draw_text_center(
            &mut display,
            &format!("Now playing {}", wavs[selection.unwrap()]),
            MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
        )
        .expect("Display Error");

        let file_name = format!("/music/{}", wavs[selection.unwrap()]);
        let file = File::new(String::from(file_name));
        let mut wav = Wav::new(file).unwrap();
        println!("sample rate: {}", wav.sample_rate());
        println!("channels: {:?}", wav.channels() as u8);

        let mut buf = [0_u8; AUDIO_BUFFER_LEN];

        loop {
            if audio_buffer_ready() {
                if wav.is_eof() {
                    break;
                }

                let _read = wav.read(&mut buf).unwrap();
                send_audio_buffer(&buf);
            }

            let event = get_key();
            if event.state == KeyState::Released {
                match event.key {
                    KeyCode::Esc => return,
                    _ => (),
                }
            }
        }
    }
}

struct File {
    current_pos: usize,
    file: String,
}

impl File {
    fn new(file: String) -> Self {
        Self {
            current_pos: 0,
            file,
        }
    }
}

impl PlatformFile for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, PlatformFileError> {
        let read = read_file(&self.file, self.current_pos, buf);
        self.current_pos += read;
        Ok(read)
    }

    fn seek_from_current(&mut self, offset: i64) -> Result<(), PlatformFileError> {
        if offset.is_positive() {
            self.current_pos += offset as usize;
        } else {
            self.current_pos -= offset as usize;
        }
        Ok(())
    }

    fn seek_from_start(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        self.current_pos = offset;
        Ok(())
    }

    fn seek_from_end(&mut self, offset: usize) -> Result<(), PlatformFileError> {
        self.current_pos = self.length() - offset;
        Ok(())
    }

    fn length(&mut self) -> usize {
        file_len(&self.file)
    }
}
