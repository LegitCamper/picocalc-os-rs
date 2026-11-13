const SECTOR_SIZE: usize = 512;

#[derive(Debug, PartialEq, Eq)]
pub enum ReadError<Error> {
    OutOfBounds,
    Source(Error),
}

/// Cache for small reads from one immutable file. Create a new cache per file.
pub struct ReadCache {
    file_size: u32,
    // ponytail: two sectors cover alternating metadata streams; expand only after profiling.
    sectors: [[u8; SECTOR_SIZE]; 2],
    offsets: [Option<u32>; 2],
    oldest: usize,
}

impl ReadCache {
    pub fn new(file_size: u32) -> Self {
        Self {
            file_size,
            sectors: [[0; SECTOR_SIZE]; 2],
            offsets: [None; 2],
            oldest: 0,
        }
    }

    /// `read_exact_at` must fill its entire buffer or return an error.
    pub fn read_at<Error>(
        &mut self,
        mut offset: u32,
        mut bytes: &mut [u8],
        mut read_exact_at: impl FnMut(u32, &mut [u8]) -> Result<(), Error>,
    ) -> Result<(), ReadError<Error>> {
        let size = u32::try_from(bytes.len()).map_err(|_| ReadError::OutOfBounds)?;
        if offset
            .checked_add(size)
            .is_none_or(|end| end > self.file_size)
        {
            return Err(ReadError::OutOfBounds);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() >= SECTOR_SIZE {
            return read_exact_at(offset, bytes).map_err(ReadError::Source);
        }
        while !bytes.is_empty() {
            let sector_offset = offset / SECTOR_SIZE as u32 * SECTOR_SIZE as u32;
            let slot = match self
                .offsets
                .iter()
                .position(|&start| start == Some(sector_offset))
            {
                Some(slot) => slot,
                None => {
                    let slot = self.oldest;
                    self.offsets[slot] = None;
                    let count = (self.file_size - sector_offset).min(SECTOR_SIZE as u32) as usize;
                    read_exact_at(sector_offset, &mut self.sectors[slot][..count])
                        .map_err(ReadError::Source)?;
                    self.offsets[slot] = Some(sector_offset);
                    slot
                }
            };
            self.oldest = 1 - slot;
            let within = (offset - sector_offset) as usize;
            let count = bytes.len().min(SECTOR_SIZE - within);
            bytes[..count].copy_from_slice(&self.sectors[slot][within..within + count]);
            offset += count as u32;
            bytes = &mut bytes[count..];
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alternating_symbols_and_names_read_each_sector_once() {
        let source: Vec<u8> = (0..32 * SECTOR_SIZE).map(|index| index as u8).collect();
        let mut cache = ReadCache::new(source.len() as u32);
        let mut reads = 0;
        let mut read = |offset: u32, bytes: &mut [u8]| {
            reads += 1;
            bytes.copy_from_slice(&source[offset as usize..offset as usize + bytes.len()]);
            Ok::<_, ()>(())
        };
        for symbol in 0..512 {
            for (offset, length) in [(symbol * 16, 16), (8192 + symbol * 16, 14)] {
                let mut bytes = [0; 16];
                cache
                    .read_at(offset, &mut bytes[..length], &mut read)
                    .unwrap();
                assert_eq!(
                    &bytes[..length],
                    &source[offset as usize..offset as usize + length]
                );
            }
        }
        assert_eq!(reads, 32);
    }

    #[test]
    fn cross_sector_reads_and_short_final_sector_match_source() {
        let source: Vec<u8> = (0..SECTOR_SIZE + 7).map(|index| index as u8).collect();
        let mut cache = ReadCache::new(source.len() as u32);
        let mut reads = Vec::new();
        let mut read = |offset: u32, bytes: &mut [u8]| {
            reads.push((offset, bytes.len()));
            bytes.copy_from_slice(&source[offset as usize..offset as usize + bytes.len()]);
            Ok::<_, ()>(())
        };
        let mut bytes = [0; 14];
        cache.read_at(505, &mut bytes, &mut read).unwrap();
        assert_eq!(bytes, source[505..]);
        cache.read_at(505, &mut bytes, &mut read).unwrap();
        assert_eq!(reads, [(0, 512), (512, 7)]);
    }

    #[test]
    fn evicts_least_recently_used_sector() {
        let mut cache = ReadCache::new(1536);
        let mut reads = Vec::new();
        for offset in [0, 512, 0, 1024, 0, 512] {
            cache
                .read_at(offset, &mut [0], |start, bytes| {
                    reads.push(start);
                    bytes.fill(0);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        assert_eq!(reads, [0, 512, 1024, 512]);
    }

    #[test]
    fn bulk_reads_bypass_cache_without_evicting_metadata() {
        let mut cache = ReadCache::new(2048);
        let mut reads = Vec::new();
        let mut read = |offset, bytes: &mut [u8]| {
            reads.push((offset, bytes.len()));
            bytes.fill(7);
            Ok::<_, ()>(())
        };
        cache.read_at(0, &mut [0], &mut read).unwrap();
        let mut bytes = [0; 1024];
        cache.read_at(513, &mut bytes, &mut read).unwrap();
        assert_eq!(bytes, [7; 1024]);
        cache.read_at(0, &mut [0], &mut read).unwrap();
        assert_eq!(reads, [(0, 512), (513, 1024)]);
    }

    #[test]
    fn invalid_ranges_and_empty_reads_do_not_touch_source() {
        let mut cache = ReadCache::new(u32::MAX);
        let mut read = |_, _: &mut [u8]| -> Result<(), ()> { panic!("unexpected read") };
        assert_eq!(
            cache.read_at(u32::MAX, &mut [0], &mut read),
            Err(ReadError::OutOfBounds)
        );
        assert_eq!(
            cache.read_at(u32::MAX - 1, &mut [0; 2], &mut read),
            Err(ReadError::OutOfBounds)
        );
        cache.read_at(u32::MAX, &mut [], &mut read).unwrap();
        let mut cache = ReadCache::new(0);
        cache.read_at(0, &mut [], &mut read).unwrap();
        assert_eq!(
            cache.read_at(1, &mut [], &mut read),
            Err(ReadError::OutOfBounds)
        );
    }

    #[test]
    fn partial_failure_never_marks_sector_valid() {
        let mut cache = ReadCache::new(1536);
        for offset in [0, 512] {
            cache
                .read_at(offset, &mut [0], |_, bytes| {
                    bytes.fill(7);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        assert_eq!(
            cache.read_at(1024, &mut [0], |_, bytes| {
                bytes[0] = 99;
                Err("read failed")
            }),
            Err(ReadError::Source("read failed"))
        );
        let mut reads = Vec::new();
        for offset in [0, 1024] {
            let mut byte = [0];
            cache
                .read_at(offset, &mut byte, |start, bytes| {
                    reads.push(start);
                    bytes.fill(7);
                    Ok::<_, ()>(())
                })
                .unwrap();
            assert_eq!(byte, [7]);
        }
        assert_eq!(reads, [0, 1024]);
        assert_eq!(
            cache.read_at(0, &mut [0; 512], |_, _| Err("bulk failed")),
            Err(ReadError::Source("bulk failed"))
        );
    }
}
