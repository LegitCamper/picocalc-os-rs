use crate::{AUDIO_BUFFER_LEN, SysError};

pub const DEFAULT_SAMPLE_RATE: u32 = 22_050;

/// Validates the integer PIO divider without overflowing the sample clock.
pub fn clock_divider(clock_hz: u32, sample_rate: u32) -> Option<u32> {
    let sample_clock = 256_u32.checked_mul(sample_rate)?;
    let divider = clock_hz.checked_div(sample_clock)?;
    (1..=u16::MAX as u32).contains(&divider).then_some(divider)
}

/// One pending packet; the renderer owns any packet removed from this mailbox.
pub struct AudioMailbox {
    pending: Option<[u8; AUDIO_BUFFER_LEN]>,
    sample_rate: u32,
    resetting: bool,
    generation: u32,
}

impl Default for AudioMailbox {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMailbox {
    pub const fn new() -> Self {
        Self {
            pending: None,
            sample_rate: DEFAULT_SAMPLE_RATE,
            resetting: false,
            generation: 0,
        }
    }

    pub fn ready(&self) -> bool {
        !self.resetting && self.pending.is_none()
    }

    pub fn submit(&mut self, samples: &[u8]) -> Result<(), SysError> {
        let samples: &[u8; AUDIO_BUFFER_LEN] =
            samples.try_into().map_err(|_| SysError::InvalidArgument)?;
        if !self.ready() {
            return Err(SysError::Busy);
        }
        self.pending = Some(*samples);
        Ok(())
    }

    pub fn configure(&mut self, clock_hz: u32, sample_rate: u32) -> Result<(), SysError> {
        clock_divider(clock_hz, sample_rate).ok_or(SysError::InvalidArgument)?;
        if self.resetting {
            return Err(SysError::Busy);
        }
        self.sample_rate = sample_rate;
        Ok(())
    }

    pub fn take(&mut self) -> Option<([u8; AUDIO_BUFFER_LEN], u32)> {
        if self.resetting {
            return None;
        }
        self.pending
            .take()
            .map(|samples| (samples, self.sample_rate))
    }

    pub fn begin_reset(&mut self) -> u32 {
        self.generation = self.generation.wrapping_add(1);
        self.resetting = true;
        self.pending = None;
        self.sample_rate = DEFAULT_SAMPLE_RATE;
        self.generation
    }

    pub fn finish_reset(&mut self, generation: u32) {
        if self.generation == generation {
            self.resetting = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_divider_boundaries_without_overflow() {
        assert_eq!(clock_divider(150_000_000, 0), None);
        assert_eq!(clock_divider(150_000_000, u32::MAX), None);
        assert_eq!(clock_divider(255, 1), None);
        assert_eq!(clock_divider(256, 1), Some(1));
        assert_eq!(clock_divider(256 * 65_535, 1), Some(65_535));
        assert_eq!(clock_divider(256 * 65_536, 1), None);
    }

    #[test]
    fn consumes_each_packet_once_and_reports_backpressure() {
        let mut mailbox = AudioMailbox::new();
        assert!(mailbox.ready());
        assert_eq!(mailbox.submit(&[1; AUDIO_BUFFER_LEN]), Ok(()));
        assert!(!mailbox.ready());
        assert_eq!(mailbox.submit(&[2; AUDIO_BUFFER_LEN]), Err(SysError::Busy));
        assert_eq!(
            mailbox.take(),
            Some(([1; AUDIO_BUFFER_LEN], DEFAULT_SAMPLE_RATE))
        );
        assert_eq!(mailbox.take(), None);
        assert!(mailbox.ready());
    }

    #[test]
    fn rejects_invalid_packet_length_without_consuming_slot() {
        let mut mailbox = AudioMailbox::new();
        assert_eq!(mailbox.submit(&[]), Err(SysError::InvalidArgument));
        assert_eq!(
            mailbox.submit(&[0; AUDIO_BUFFER_LEN - 1]),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            mailbox.submit(&[0; AUDIO_BUFFER_LEN + 1]),
            Err(SysError::InvalidArgument)
        );
        assert!(mailbox.ready());
    }

    #[test]
    fn reset_discards_pending_packet_and_restores_default_rate() {
        let mut mailbox = AudioMailbox::new();
        assert_eq!(mailbox.configure(150_000_000, 44_100), Ok(()));
        assert_eq!(mailbox.submit(&[1; AUDIO_BUFFER_LEN]), Ok(()));
        let generation = mailbox.begin_reset();
        assert_eq!(mailbox.take(), None);
        assert!(!mailbox.ready());
        assert_eq!(mailbox.submit(&[2; AUDIO_BUFFER_LEN]), Err(SysError::Busy));
        assert_eq!(mailbox.configure(150_000_000, 44_100), Err(SysError::Busy));
        mailbox.finish_reset(generation);
        assert_eq!(mailbox.submit(&[2; AUDIO_BUFFER_LEN]), Ok(()));
        assert_eq!(
            mailbox.take(),
            Some(([2; AUDIO_BUFFER_LEN], DEFAULT_SAMPLE_RATE))
        );
    }

    #[test]
    fn stale_reset_acknowledgement_cannot_unlock_new_session() {
        let mut mailbox = AudioMailbox::new();
        let old_generation = mailbox.begin_reset();
        let current_generation = mailbox.begin_reset();
        mailbox.finish_reset(old_generation);
        assert!(!mailbox.ready());
        mailbox.finish_reset(current_generation);
        assert!(mailbox.ready());
    }

    #[test]
    fn invalid_rate_preserves_previous_configuration() {
        let mut mailbox = AudioMailbox::new();
        assert_eq!(mailbox.configure(150_000_000, 44_100), Ok(()));
        assert_eq!(
            mailbox.configure(150_000_000, 0),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(mailbox.submit(&[3; AUDIO_BUFFER_LEN]), Ok(()));
        assert_eq!(mailbox.take().map(|(_, rate)| rate), Some(44_100));
    }
}
