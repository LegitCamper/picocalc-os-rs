pub struct FramePacer {
    deadline_ms: u64,
    last_present_ms: Option<u64>,
}

impl FramePacer {
    pub fn new(now_ms: u64) -> Self {
        Self {
            deadline_ms: now_ms,
            last_present_ms: None,
        }
    }

    pub fn advance(&mut self, duration_ms: u64) {
        self.deadline_ms = self.deadline_ms.saturating_add(duration_ms.max(1));
    }

    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        self.deadline_ms.saturating_sub(now_ms)
    }

    pub fn should_present(&self, now_ms: u64) -> bool {
        // ponytail: refresh stale output every 100 ms; faster CPU-bound playback needs faster decoding.
        self.remaining_ms(now_ms) != 0
            || self
                .last_present_ms
                .is_none_or(|last| now_ms.saturating_sub(last) >= 100)
    }

    pub fn presented(&mut self, now_ms: u64) {
        self.last_present_ms = Some(now_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_writes_skip_expired_frames_without_moving_deadlines() {
        let mut pacer = FramePacer::new(1_000);
        pacer.advance(20);
        assert!(pacer.should_present(1_005));
        pacer.presented(1_065);
        pacer.advance(20);
        assert!(!pacer.should_present(1_066));
        pacer.advance(20);
        assert!(!pacer.should_present(1_067));
        pacer.advance(20);
        assert!(pacer.should_present(1_068));
        assert_eq!(pacer.remaining_ms(1_068), 12);
    }

    #[test]
    fn slow_decoding_still_presents_first_frame_and_makes_progress() {
        let mut pacer = FramePacer::new(0);
        pacer.advance(10);
        assert!(pacer.should_present(30));
        pacer.presented(40);
        pacer.advance(10);
        assert!(!pacer.should_present(139));
        assert!(pacer.should_present(140));
    }

    #[test]
    fn zero_delays_and_clock_limits_do_not_wrap() {
        let mut pacer = FramePacer::new(5);
        pacer.advance(0);
        assert_eq!(pacer.remaining_ms(5), 1);
        pacer.advance(u64::MAX);
        assert_eq!(pacer.remaining_ms(u64::MAX), 0);
    }

    #[test]
    fn decode_estimate_can_skip_a_frame_that_would_miss_its_deadline() {
        let mut pacer = FramePacer::new(100);
        pacer.presented(100);
        pacer.advance(20);
        assert!(pacer.should_present(110));
        assert!(!pacer.should_present(110 + 15));
        pacer.advance(20);
        assert!(pacer.should_present(110 + 15));
    }

    #[test]
    fn copy_cost_is_included_when_skipping_to_current_frame() {
        let mut pacer = FramePacer::new(0);
        pacer.presented(0);
        let now = 20;
        let decode_ms = 10;
        let copy_ms = 25;
        pacer.advance(33);
        assert!(pacer.should_present(now + decode_ms));
        assert!(!pacer.should_present(now + decode_ms + copy_ms));
        pacer.advance(33);
        assert!(pacer.should_present(now + decode_ms + copy_ms));
        assert_eq!(pacer.remaining_ms(now + decode_ms + copy_ms), 11);
    }
}
