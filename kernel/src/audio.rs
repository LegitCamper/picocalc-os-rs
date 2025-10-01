use core::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use embassy_rp::{
    Peri,
    pio::Pio,
    pio_programs::pwm::{PioPwm, PioPwmProgram},
    pwm::{Config, Pwm, SetDutyCycle},
};
use embassy_time::Timer;

use crate::{Audio, Irqs};

const AUDIO_BUFFER_LEN: usize = 1024;
const _: () = assert!(AUDIO_BUFFER_LEN == abi_sys::AUDIO_BUFFER_LEN);
pub static mut AUDIO_BUFFER: [u8; AUDIO_BUFFER_LEN] = [0; AUDIO_BUFFER_LEN];
static mut AUDIO_BUFFER_1: [u8; AUDIO_BUFFER_LEN] = [0; AUDIO_BUFFER_LEN];

pub static AUDIO_BUFFER_READY: AtomicBool = AtomicBool::new(true);

pub const SAMPLE_RATE_HZ: u32 = 22_050;

#[embassy_executor::task]
pub async fn audio_handler(audio: Audio) {
    let var_name = Pio::new(audio.pio_left, Irqs);
    let Pio {
        mut common, sm0, ..
    } = var_name;

    let prg = PioPwmProgram::new(&mut common);
    let mut pwm_pio = PioPwm::new(&mut common, sm0, audio.left, &prg);

    let period = Duration::from_nanos(1_000_000_000 / SAMPLE_RATE_HZ as u64);
    pwm_pio.set_period(period);

    pwm_pio.start();

    let sample_interval = 1_000_000 / SAMPLE_RATE_HZ as u64; // in µs ≈ 45 µs

    loop {
        for &sample in unsafe { &AUDIO_BUFFER }.iter() {
            let period_ns = period.as_nanos() as u32;
            let duty_ns = period_ns * (sample as u32) / 255;
            pwm_pio.write(Duration::from_nanos(duty_ns as u64));
            Timer::after_micros(sample_interval).await; // sample interval = 1 / sample rate
        }

        unsafe { core::mem::swap(&mut AUDIO_BUFFER, &mut AUDIO_BUFFER_1) };
        AUDIO_BUFFER_READY.store(true, Ordering::Release)
    }
}
