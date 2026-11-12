pub fn remaining_ms(now: u64, last_step: u64) -> u64 {
    125_u64.saturating_sub(now.saturating_sub(last_step))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movement_waits_for_elapsed_time_without_catchup_bursts() {
        let mut last_step = 1000;
        assert_eq!(remaining_ms(1000, last_step), 125);
        assert_eq!(remaining_ms(1124, last_step), 1);
        assert_eq!(remaining_ms(1125, last_step), 0);
        assert_eq!(remaining_ms(5000, last_step), 0);
        last_step = 5000;
        assert_eq!(remaining_ms(5000, last_step), 125);
        assert_eq!(remaining_ms(4999, last_step), 125);
    }
}
