use core::ops::Range;

pub fn scan_band_delay_us(
    frame_us: u64,
    counter_period: u16,
    scanline: u16,
    rows: Range<usize>,
) -> u64 {
    if !(5_000..=100_000).contains(&frame_us)
        || rows.start >= rows.end
        || rows.end > 320
        || counter_period < 2
        || scanline >= counter_period
    {
        return 0;
    }
    let phase_us = frame_us * u64::from(scanline) / u64::from(counter_period);
    // ponytail: assumes ML=0 and 480+2+2 scan rows; cap delay until physical row mapping is verified.
    let target_us = frame_us * (rows.start as u64 + 2) / 484 + 250;
    let delay_us = (target_us + frame_us - phase_us) % frame_us;
    if delay_us <= 500 { delay_us } else { 0 }
}

/// Takes the next contiguous dirty run within one tile row.
pub fn take_dirty_run(tiles: &mut [bool], start: usize) -> Option<Range<usize>> {
    let first = start.checked_add(tiles.get(start..)?.iter().position(|dirty| *dirty)?)?;
    let end = first
        + tiles[first..]
            .iter()
            .position(|dirty| !dirty)
            .unwrap_or(tiles.len() - first);
    tiles[first..end].fill(false);
    Some(first..end)
}

#[cfg(test)]
mod tests {
    use super::{scan_band_delay_us, take_dirty_run};

    #[test]
    fn scan_delay_never_waits_a_frame_or_rejects_late_updates() {
        assert_eq!(scan_band_delay_us(17_000, 242, 0, 0..320), 320);
        assert_eq!(scan_band_delay_us(17_000, 242, 1, 0..320), 250);
        assert_eq!(scan_band_delay_us(17_000, 242, 100, 0..320), 0);
        for scanline in 0..242 {
            for first in (0..320).step_by(16) {
                assert!(scan_band_delay_us(17_000, 242, scanline, first..320) <= 500);
            }
        }
        for (frame, period, scanline) in [
            (0, 242, 0),
            (u64::MAX, 242, 0),
            (17_000, 0, 0),
            (17_000, 242, 242),
        ] {
            assert_eq!(scan_band_delay_us(frame, period, scanline, 0..320), 0);
        }
        assert_eq!(scan_band_delay_us(17_000, 242, 0, 0..321), 0);
    }

    #[test]
    fn consumes_each_run_without_crossing_clean_tiles() {
        let mut tiles = [false, true, true, false, true];
        assert_eq!(take_dirty_run(&mut tiles, 0), Some(1..3));
        assert_eq!(tiles, [false, false, false, false, true]);
        assert_eq!(take_dirty_run(&mut tiles, 3), Some(4..5));
        assert_eq!(take_dirty_run(&mut tiles, 0), None);
    }

    #[test]
    fn retry_restores_only_failed_run() {
        let mut tiles = [true, true, false, true];
        let failed = take_dirty_run(&mut tiles, 0).unwrap();
        tiles[failed.clone()].fill(true);
        assert_eq!(take_dirty_run(&mut tiles, 0), Some(failed));
        assert_eq!(take_dirty_run(&mut tiles, 2), Some(3..4));
        assert_eq!(take_dirty_run(&mut tiles, usize::MAX), None);
    }
}
