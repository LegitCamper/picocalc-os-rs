use crate::Audio;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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

// atomics for user applications to signal changes to audio buffers
pub static AUDIO_BUFFER_READY: AtomicBool = AtomicBool::new(true);
pub static AUDIO_BUFFER_WRITTEN: AtomicBool = AtomicBool::new(false);
pub static AUDIO_BUFFER_SAMPLE_RATE: AtomicU32 = AtomicU32::new(SAMPLE_RATE_HZ);

/// resets audio buffers after user applications are unloaded
pub fn clear_audio_buffers() {
    unsafe {
        AUDIO_BUFFER.fill(SILENCE);
        AUDIO_BUFFER_1.fill(SILENCE);
    }
}

#[embassy_executor::task]
pub async fn audio_handler(mut audio: Audio) {
    let prg = PioPwmAudioProgram8Bit::new(&mut audio.pio);
    let mut pwm_pio_left =
        PioPwmAudio::new(audio.dma0, &mut audio.pio, audio.sm0, audio.left, &prg);
    let mut pwm_pio_right =
        PioPwmAudio::new(audio.dma1, &mut audio.pio, audio.sm1, audio.right, &prg);

    // enable sms at the same time to ensure they are synced
    audio.pio.apply_sm_batch(|pio| {
        pio.set_enable(&mut pwm_pio_right.sm, true);
        pio.set_enable(&mut pwm_pio_left.sm, true);
    });

    let mut sample_rate = SAMPLE_RATE_HZ;

    loop {
        unsafe {
            let new_sample_rate = AUDIO_BUFFER_SAMPLE_RATE.load(Ordering::Acquire);
            if new_sample_rate != sample_rate {
                sample_rate = new_sample_rate;
                pwm_pio_left.reconfigure(sample_rate);
                pwm_pio_right.reconfigure(sample_rate);

                // restart sms at the same time to ensure they are synced
                audio.pio.apply_sm_batch(|pio| {
                    pio.restart(&mut pwm_pio_right.sm);
                    pio.restart(&mut pwm_pio_left.sm);
                });
            }

            if AUDIO_BUFFER_WRITTEN.load(Ordering::Acquire) {
                write_samples(&mut pwm_pio_left, &mut pwm_pio_right, &AUDIO_BUFFER_1).await;
                AUDIO_BUFFER_1.fill(SILENCE);
                core::mem::swap(&mut AUDIO_BUFFER, &mut AUDIO_BUFFER_1);
                AUDIO_BUFFER_READY.store(true, Ordering::Release)
            } else {
                yield_now().await;
            }
        }
    }
}

async fn write_samples<PIO: Instance>(
    left: &mut PioPwmAudio<'static, PIO, 0>,
    right: &mut PioPwmAudio<'static, PIO, 1>,
    buf: &[u8],
) {
    // pack two samples per word
    static mut PACKED_BUF_L: [u32; AUDIO_BUFFER_SAMPLES / 2] = [0; AUDIO_BUFFER_SAMPLES / 2];
    static mut PACKED_BUF_R: [u32; AUDIO_BUFFER_SAMPLES / 2] = [0; AUDIO_BUFFER_SAMPLES / 2];

    unsafe {
        for ((pl, pr), sample) in PACKED_BUF_L
            .iter_mut()
            .zip(PACKED_BUF_R.iter_mut())
            .zip(buf.chunks(4))
        {
            *pl = pack_u8_samples(sample[0], sample[2]);
            *pr = pack_u8_samples(sample[1], sample[3]);
        }

        let left_fut = left
            .sm
            .tx()
            .dma_push(left.dma.reborrow(), &PACKED_BUF_L, false);

        let right_fut = right
            .sm
            .tx()
            .dma_push(right.dma.reborrow(), &PACKED_BUF_R, false);

        join(left_fut, right_fut).await;
    }
}

struct PioPwmAudioProgram8Bit<'d, PIO: Instance>(LoadedProgram<'d, PIO>);

/// Writes one sample to pwm as high and low time
impl<'d, PIO: Instance> PioPwmAudioProgram8Bit<'d, PIO> {
    fn new(common: &mut Common<'d, PIO>) -> Self {
        // only uses 16 bits top for pwm high, bottom for pwm low
        // allows storing two samples per word
        let prg = pio_asm!(
            ".side_set 1",

            "set x, 0 side 1",
            "set y, 0 side 0",
            ".wrap_target",

            "check:",
            "pull ifempty noblock side 1", // gets new osr or loads 0 into x, gets second sample if osr not empty
            "out x, 8 side 0", // pwm high time
            "out y, 8 side 1", // pwm low time
            "jmp x!=y play_sample side 0", // x & y are never equal unless osr was empty
            "set x, 1 side 1",
            "set y, 1 side 0",

            "play_sample:"
            "loop_high:",
            "jmp x-- loop_high side 1", // keep pwm high, decrement X until 0
            "loop_low:",
            "jmp y-- loop_low side 0", // keep pwm low, decrement Y until 0
            ".wrap"
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
    fn get_sm_divider(sample_rate: u32) -> u32 {
        let target_clock = (u8::MAX as u32 + 1) * sample_rate;
        clk_sys_freq() / target_clock
    }

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
        cfg.fifo_join = FifoJoin::TxOnly;
        let shift_cfg = ShiftConfig {
            auto_fill: false,
            ..Default::default()
        };
        cfg.shift_out = shift_cfg;
        cfg.use_program(&prg.0, &[&pin]);
        sm.set_config(&cfg);

        sm.set_clock_divider(Self::get_sm_divider(SAMPLE_RATE_HZ).to_fixed());

        Self {
            dma: dma.into(),
            sm,
        }
    }

    fn reconfigure(&mut self, sample_rate: u32) {
        self.sm
            .set_clock_divider(Self::get_sm_divider(sample_rate).to_fixed());
    }
}

/// packs two u8 samples into 32bit word
fn pack_u8_samples(sample1: u8, sample2: u8) -> u32 {
    (u8_pcm_to_pwm(sample1) as u32) << 16 | u8_pcm_to_pwm(sample2) as u32
}

fn u8_pcm_to_pwm(sample: u8) -> u16 {
    ((sample as u16) << 8) | ((u8::MAX - sample) as u16)
}
