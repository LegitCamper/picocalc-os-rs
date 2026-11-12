use core::ops::Range;

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
    use super::take_dirty_run;

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
