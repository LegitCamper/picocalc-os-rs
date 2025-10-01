#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    AUDIO_BUFFER_LEN, KeyCode, KeyState, Rng, audio_buffer_ready,
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
    file_len, get_key, lock_display, print, read_file, send_audio_buffer, sleep,
};
use alloc::{format, string::String};
use core::panic::PanicInfo;
use embedded_audio::{AudioFile, PlatformFile, PlatformFileError, wav::Wav};
use embedded_graphics::{pixelcolor::Rgb565, prelude::RgbColor};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    print(&format!(
        "user panic: {} @ {:?}",
        info.message(),
        info.location(),
    ));
    loop {}
}

#[unsafe(no_mangle)]
pub extern "Rust" fn _start() {
    main()
}

pub fn main() {
    print("Starting Wav player app");
    let mut display = Display;

    let mut buf = [0_u8; AUDIO_BUFFER_LEN];

    let file = File::new(String::from("/music/test.wav"));
    let mut wav = Wav::new(file).unwrap();
    loop {
        if audio_buffer_ready() {
            if wav.is_eof() {
                wav.restart().unwrap()
            }

            let read = wav.read(&mut buf).unwrap();
            send_audio_buffer(&buf);
        }

        let event = get_key();
        if event.state != KeyState::Idle {
            match event.key {
                KeyCode::Esc => return,
                _ => (),
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
