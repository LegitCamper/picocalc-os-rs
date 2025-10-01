use crate::Audio;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_rp::{
    Peri,
    dma::{AnyChannel, Channel},
    gpio::Level,
    pio::{
        Common, Config, Direction, FifoJoin, Instance, LoadedProgram, PioPin, ShiftConfig,
        ShiftDirection, StateMachine, program::pio_asm,
    },
    pio_programs::clock_divider::calculate_pio_clock_divider,
};
use embassy_time::Timer;

const AUDIO_BUFFER_LEN: usize = 1024;
const _: () = assert!(AUDIO_BUFFER_LEN == abi_sys::AUDIO_BUFFER_LEN);
pub static mut AUDIO_BUFFER: [u8; AUDIO_BUFFER_LEN] = [0; AUDIO_BUFFER_LEN];
static mut AUDIO_BUFFER_1: [u8; AUDIO_BUFFER_LEN] = [0; AUDIO_BUFFER_LEN];

pub static AUDIO_BUFFER_READY: AtomicBool = AtomicBool::new(true);

pub const SAMPLE_RATE_HZ: u32 = 8_000;

#[embassy_executor::task]
pub async fn audio_handler(mut audio: Audio) {
    let prg = PioPwmAudioProgram8Bit::new(&mut audio.pio);
    defmt::info!("loaded prg");

    let mut pwm_pio = PioPwmAudio::new(audio.dma, &mut audio.pio, audio.sm0, audio.left, &prg);
    defmt::info!("cfgd sm");

    pwm_pio.configure(SAMPLE_RATE_HZ);
    pwm_pio.start();
    loop {
        pwm_pio.write(unsafe { &AUDIO_BUFFER_1 }).await;

        unsafe { core::mem::swap(&mut AUDIO_BUFFER, &mut AUDIO_BUFFER_1) };
        AUDIO_BUFFER_READY.store(true, Ordering::Release)
    }
}

struct PioPwmAudioProgram8Bit<'d, PIO: Instance>(LoadedProgram<'d, PIO>);

impl<'d, PIO: Instance> PioPwmAudioProgram8Bit<'d, PIO> {
    fn new(common: &mut Common<'d, PIO>) -> Self {
        let prg = pio_asm!(
            "out x, 8", // samples <<
            "out y, 8", // samples max >>
            "loop_high:",
            "set pins, 1",       // keep pin high
            "jmp x-- loop_high", // decrement X until 0
            "loop_low:",
            "set pins, 0",      // keep pin low
            "jmp y-- loop_low", // decrement Y until 0
        );

        let prg = common.load_program(&prg.program);

        Self(prg)
    }
}

struct PioPwmAudio<'d, PIO: Instance, const SM: usize> {
    dma: Peri<'d, AnyChannel>,
    cfg: Config<'d, PIO>,
    sm: StateMachine<'d, PIO, SM>,
}

impl<'d, PIO: Instance, const SM: usize> PioPwmAudio<'d, PIO, SM> {
    fn new(
        dma: Peri<'d, impl Channel>,
        pio: &mut Common<'d, PIO>,
        mut sm: StateMachine<'d, PIO, SM>,
        pin: Peri<'d, impl PioPin>,
        prg: &PioPwmAudioProgram8Bit<'d, PIO>,
    ) -> Self {
        let pin = pio.make_pio_pin(pin);
        sm.set_pins(Level::High, &[&pin]);
        sm.set_pin_dirs(Direction::Out, &[&pin]);

        let mut cfg = Config::default();
        cfg.set_set_pins(&[&pin]);
        cfg.fifo_join = FifoJoin::TxOnly;
        let mut shift_cfg = ShiftConfig::default();
        shift_cfg.auto_fill = true;
        cfg.shift_out = shift_cfg;
        cfg.use_program(&prg.0, &[]);
        sm.set_config(&cfg);

        Self {
            dma: dma.into(),
            cfg,
            sm,
        }
    }

    fn configure(&mut self, sample_rate: u32) {
        let cycles_per_sample = u8::MAX as u32 + 2; // X_max + Y_max + movs
        let target_pio_hz = cycles_per_sample * sample_rate; // ~11.3 MHz

        let divider = calculate_pio_clock_divider(target_pio_hz);
        self.cfg.clock_divider = divider;
        self.sm.set_clock_divider(divider);
    }

    async fn write(&mut self, buf: &[u8]) {
        let mut packed_buf = [0_u32; AUDIO_BUFFER_LEN / 4];

        for (packed_sample, sample) in packed_buf.iter_mut().zip(buf.chunks(2)) {
            *packed_sample = pack_two_samples(sample[0], sample[1]);
        }

        self.sm
            .tx()
            .dma_push(self.dma.reborrow(), &packed_buf, false)
            .await
    }

    fn start(&mut self) {
        self.sm.set_enable(true);
    }

    fn stop(&mut self) {
        self.sm.set_enable(false);
    }
}

fn pack_two_samples(s1: u8, s2: u8) -> u32 {
    let x = ((s1 as u16) << 8 | (255 - s1) as u16); // original
    let y = ((s2 as u16) << 8 | (255 - s2) as u16);

    // Scale to full 16-bit for higher volume:
    let x_scaled = ((x as u32) << 8) | (x as u32 >> 8);
    let y_scaled = ((y as u32) << 8) | (y as u32 >> 8);

    (x_scaled << 16) | y_scaled
}
