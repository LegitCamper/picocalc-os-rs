#![no_std]
#![no_main]

extern crate alloc;
use abi::{
    audio::{AUDIO_BUFFER_LEN, audio_buffer_ready, send_audio_buffer},
    display::Display,
    fs::{file_len, read_file},
    get_key,
    keyboard::{KeyCode, KeyState},
    println,
};
use alloc::string::String;
use core::panic::PanicInfo;
use embedded_audio::{AudioFile, PlatformFile, PlatformFileError, wav::Wav};

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
    let mut _display = Display::take();

    let mut buf = [0_u8; AUDIO_BUFFER_LEN];

    let file = File::new(String::from("/music/test.wav"));
    let mut wav = Wav::new(file).unwrap();
    loop {
        if audio_buffer_ready() {
            if wav.is_eof() {
                wav.restart().unwrap()
            }

            let _read = wav.read(&mut buf).unwrap();
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
