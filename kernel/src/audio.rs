use crate::Audio;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_futures::{join::join, yield_now};
use embassy_rp::{
    Peri,
    clocks::clk_sys_freq,
    dma::{AnyChannel, Channel},
    gpio::Level,
    pio::{
        Common, Config, Direction, FifoJoin, Instance, LoadedProgram, PioPin, ShiftConfig,
        StateMachine, program::pio_asm,
    },
};
use fixed::traits::ToFixed;

pub const SAMPLE_RATE_HZ: u32 = 22_050;
const AUDIO_BUFFER_SAMPLES: usize = 1024;
const _: () = assert!(AUDIO_BUFFER_SAMPLES == userlib_sys::AUDIO_BUFFER_SAMPLES);

const SILENCE: u8 = u8::MAX / 2;

// 8bit stereo interleaved PCM audio buffers
pub static mut AUDIO_BUFFER: [u8; AUDIO_BUFFER_SAMPLES * 2] = [SILENCE; AUDIO_BUFFER_SAMPLES * 2];
static mut AUDIO_BUFFER_1: [u8; AUDIO_BUFFER_SAMPLES * 2] = [SILENCE; AUDIO_BUFFER_SAMPLES * 2];

pub static AUDIO_BUFFER_READY: AtomicBool = AtomicBool::new(true);

#[embassy_executor::task]
pub async fn audio_handler(mut audio: Audio) {
    let prg = PioPwmAudioProgram8Bit::new(&mut audio.pio);
    let mut pwm_pio_left =
        PioPwmAudio::new(audio.dma0, &mut audio.pio, audio.sm0, audio.left, &prg);
    let mut pwm_pio_right =
        PioPwmAudio::new(audio.dma1, &mut audio.pio, audio.sm1, audio.right, &prg);

    loop {
        unsafe {
            // if AUDIO_BUFFER.iter().any(|s| *s != SILENCE) {
            write_samples(&mut pwm_pio_left, &mut pwm_pio_right, &AUDIO_BUFFER_1).await;
            AUDIO_BUFFER_1.fill(SILENCE);
            core::mem::swap(&mut AUDIO_BUFFER, &mut AUDIO_BUFFER_1);
            AUDIO_BUFFER_READY.store(true, Ordering::Release)
            // } else {
            //     yield_now().await;
            // }
        }
    }
}

async fn write_samples<PIO: Instance>(
    left: &mut PioPwmAudio<'static, PIO, 0>,
    right: &mut PioPwmAudio<'static, PIO, 1>,
    buf: &[u8],
) {
    // pack two samples per word
    let mut packed_buf_left: [u32; AUDIO_BUFFER_SAMPLES / 2] = [0; AUDIO_BUFFER_SAMPLES / 2];
    let mut packed_buf_right: [u32; AUDIO_BUFFER_SAMPLES / 2] = [0; AUDIO_BUFFER_SAMPLES / 2];

    for ((pl, pr), sample) in packed_buf_left
        .iter_mut()
        .zip(packed_buf_right.iter_mut())
        .zip(buf.chunks(4))
    {
        *pl = pack_u8_samples(sample[0], sample[2]);
        *pr = pack_u8_samples(sample[1], sample[3]);
    }

    let left_fut = left
        .sm
        .tx()
        .dma_push(left.dma.reborrow(), &packed_buf_left, false);

    let right_fut = right
        .sm
        .tx()
        .dma_push(right.dma.reborrow(), &packed_buf_right, false);

    join(left_fut, right_fut).await;
}

struct PioPwmAudioProgram8Bit<'d, PIO: Instance>(LoadedProgram<'d, PIO>);

/// Writes one sample to pwm as high and low time
impl<'d, PIO: Instance> PioPwmAudioProgram8Bit<'d, PIO> {
    fn new(common: &mut Common<'d, PIO>) -> Self {
        // only uses 16 bits top for pwm high, bottom for pwm low
        // allows storing two samples per word
        let prg = pio_asm!(
            ".side_set 1",

            "check:",
            // "set x, 0 side 1",
            // "set y, 0 side 0",
            "pull ifempty noblock side 1", // gets new osr or loads 0 into x, gets second sample if osr not empty
            "out x, 8 side 0", // pwm high time
            "out y, 8 side 1", // pwm low time
            "jmp x!=y play_sample side 0", // x & y are never equal unless osr was empty
            // play silence for 10 cycles
            "set x, 5 side 1",
            "set y, 5 side 0",

            "play_sample:"
            "loop_high:",
            "jmp x-- loop_high side 1", // keep pwm high, decrement X until 0
            "loop_low:",
            "jmp y-- loop_low side 0", // keep pwm low, decrement Y until 0
        );

        let prg = common.load_program(&prg.program);

        Self(prg)
    }
}

struct PioPwmAudio<'d, PIO: Instance, const SM: usize> {
    dma: Peri<'d, AnyChannel>,
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
        let shift_cfg = ShiftConfig {
            auto_fill: false,
            ..Default::default()
        };
        cfg.shift_out = shift_cfg;
        cfg.use_program(&prg.0, &[&pin]);
        sm.set_config(&cfg);

        let target_clock = (u8::MAX as u32 + 1) * SAMPLE_RATE_HZ;
        let divider = (clk_sys_freq() / target_clock).to_fixed();
        sm.set_clock_divider(divider);

        sm.set_enable(true);

        Self {
            dma: dma.into(),
            sm,
        }
    }
}

/// packs two u8 samples into 32bit word
fn pack_u8_samples(sample1: u8, sample2: u8) -> u32 {
    (u8_pcm_to_pwm(sample1) as u32) << 16 | u8_pcm_to_pwm(sample2) as u32
}

fn u8_pcm_to_pwm(sample: u8) -> u16 {
    ((sample as u16) << 8) | ((u8::MAX - sample) as u16)
}
