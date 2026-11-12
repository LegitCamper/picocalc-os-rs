use crate::Audio;
use core::cell::RefCell;
use embassy_futures::{
    join::join,
    select::{Either, select},
};
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
use embassy_sync::{
    blocking_mutex::{Mutex, raw::CriticalSectionRawMutex},
    signal::Signal,
};
use fixed::traits::ToFixed;
use userlib_sys::{
    SysError,
    audio::{AudioMailbox, clock_divider},
};

pub const SAMPLE_RATE_HZ: u32 = userlib_sys::audio::DEFAULT_SAMPLE_RATE;
const AUDIO_BUFFER_SAMPLES: usize = 1024;
const _: () = assert!(AUDIO_BUFFER_SAMPLES == userlib_sys::AUDIO_BUFFER_SAMPLES);

const SILENCE: u8 = u8::MAX / 2;

static MAILBOX: Mutex<CriticalSectionRawMutex, RefCell<AudioMailbox>> =
    Mutex::new(RefCell::new(AudioMailbox::new()));
static WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static RESET: Signal<CriticalSectionRawMutex, u32> = Signal::new();
static RESET_DONE: Signal<CriticalSectionRawMutex, u32> = Signal::new();

pub fn ready() -> bool {
    MAILBOX.lock(|mailbox| mailbox.borrow().ready())
}

pub fn submit(samples: &[u8]) -> Result<(), SysError> {
    MAILBOX.lock(|mailbox| mailbox.borrow_mut().submit(samples))?;
    WAKE.signal(());
    Ok(())
}

pub fn configure(sample_rate: u32) -> Result<(), SysError> {
    MAILBOX.lock(|mailbox| mailbox.borrow_mut().configure(clk_sys_freq(), sample_rate))
}

/// Called by the sole userland task after an app has stopped producing samples.
pub async fn reset() {
    let generation = MAILBOX.lock(|mailbox| mailbox.borrow_mut().begin_reset());
    RESET.signal(generation);
    while RESET_DONE.wait().await != generation {}
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

    loop {
        let pending = MAILBOX.lock(|mailbox| mailbox.borrow_mut().take());
        let reset_generation = if let Some((samples, sample_rate)) = pending {
            pwm_pio_left.reconfigure(sample_rate);
            pwm_pio_right.reconfigure(sample_rate);
            match select(
                RESET.wait(),
                write_samples(&mut pwm_pio_left, &mut pwm_pio_right, &samples),
            )
            .await
            {
                Either::First(generation) => Some(generation),
                Either::Second(()) => None,
            }
        } else {
            match select(RESET.wait(), WAKE.wait()).await {
                Either::First(generation) => Some(generation),
                Either::Second(()) => None,
            }
        };
        if let Some(generation) = reset_generation {
            pwm_pio_left.reset_output();
            pwm_pio_right.reset_output();
            audio.pio.apply_sm_batch(|pio| {
                pio.restart(&mut pwm_pio_right.sm);
                pio.restart(&mut pwm_pio_left.sm);
                pio.set_enable(&mut pwm_pio_right.sm, true);
                pio.set_enable(&mut pwm_pio_left.sm, true);
            });
            MAILBOX.lock(|mailbox| mailbox.borrow_mut().finish_reset(generation));
            RESET_DONE.signal(generation);
        }
    }
}

async fn write_samples<PIO: Instance>(
    left: &mut PioPwmAudio<'static, PIO, 0>,
    right: &mut PioPwmAudio<'static, PIO, 1>,
    buf: &[u8],
) {
    let mut packed_left = [0; AUDIO_BUFFER_SAMPLES / 2];
    let mut packed_right = [0; AUDIO_BUFFER_SAMPLES / 2];
    for ((left_word, right_word), sample) in packed_left
        .iter_mut()
        .zip(packed_right.iter_mut())
        .zip(buf.chunks_exact(4))
    {
        *left_word = pack_u8_samples(sample[0], sample[2]);
        *right_word = pack_u8_samples(sample[1], sample[3]);
    }
    let left_fut = left
        .sm
        .tx()
        .dma_push(left.dma.reborrow(), &packed_left, false);
    let right_fut = right
        .sm
        .tx()
        .dma_push(right.dma.reborrow(), &packed_right, false);
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
    program_start: u8,
}

impl<'d, PIO: Instance, const SM: usize> PioPwmAudio<'d, PIO, SM> {
    fn get_sm_divider(sample_rate: u32) -> u32 {
        clock_divider(clk_sys_freq(), sample_rate).unwrap_or(1)
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
            program_start: prg.0.origin,
        }
    }

    fn reconfigure(&mut self, sample_rate: u32) {
        self.sm
            .set_clock_divider(Self::get_sm_divider(sample_rate).to_fixed());
    }

    fn reset_output(&mut self) {
        self.sm.set_enable(false);
        self.sm.clear_fifos();
        self.sm.restart();
        self.reconfigure(SAMPLE_RATE_HZ);
        // SAFETY: the state machine is disabled, and the target is its retained loaded program.
        unsafe { self.sm.exec_jmp(self.program_start) };
        let _ = self.sm.tx().try_push(pack_u8_samples(SILENCE, SILENCE));
    }
}

/// packs two u8 samples into 32bit word
fn pack_u8_samples(sample1: u8, sample2: u8) -> u32 {
    (u8_pcm_to_pwm(sample1) as u32) << 16 | u8_pcm_to_pwm(sample2) as u32
}

fn u8_pcm_to_pwm(sample: u8) -> u16 {
    ((sample as u16) << 8) | ((u8::MAX - sample) as u16)
}
