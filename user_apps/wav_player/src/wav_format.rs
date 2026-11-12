#[derive(Debug, PartialEq, Eq)]
pub struct WavFormat {
    pub sample_rate: u32,
    pub data_start: usize,
    pub data_len: usize,
}

pub fn read_exact_at(
    read: &mut impl FnMut(usize, &mut [u8]) -> Result<usize, ()>,
    mut offset: usize,
    mut output: &mut [u8],
) -> Result<(), ()> {
    while !output.is_empty() {
        let count = read(offset, output)?;
        if count == 0 || count > output.len() {
            return Err(());
        }
        offset = offset.checked_add(count).ok_or(())?;
        output = &mut output[count..];
    }
    Ok(())
}

pub fn parse(
    length: usize,
    mut read: impl FnMut(usize, &mut [u8]) -> Result<usize, ()>,
) -> Result<WavFormat, ()> {
    let mut header = [0; 12];
    read_exact_at(&mut read, 0, &mut header)?;
    let declared = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" || declared.checked_add(8) != Some(length)
    {
        return Err(());
    }
    let mut sample_rate = None;
    let mut data = None;
    let mut position = 12;
    let mut chunks = 0;
    while position < length {
        chunks += 1;
        // ponytail: at most 64 metadata chunks; raise after bounding parser work on-device.
        if chunks > 64 || length - position < 8 {
            return Err(());
        }
        let mut chunk = [0; 8];
        read_exact_at(&mut read, position, &mut chunk)?;
        position += 8;
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as usize;
        let end = position
            .checked_add(size)
            .and_then(|end| end.checked_add(size % 2))
            .filter(|&end| end <= length)
            .ok_or(())?;
        if &chunk[..4] == b"fmt " {
            if sample_rate.is_some() || size < 16 {
                return Err(());
            }
            let mut format = [0; 16];
            read_exact_at(&mut read, position, &mut format)?;
            let rate = u32::from_le_bytes([format[4], format[5], format[6], format[7]]);
            let byte_rate = u32::from_le_bytes([format[8], format[9], format[10], format[11]]);
            if format[..4] != [1, 0, 2, 0]
                || format[12..] != [2, 0, 8, 0]
                || !(1..=48_000).contains(&rate)
                || byte_rate != rate * 2
            {
                return Err(());
            }
            sample_rate = Some(rate);
        } else if &chunk[..4] == b"data" {
            if data.is_some() || size % 2 != 0 {
                return Err(());
            }
            data = Some((position, size));
        }
        position = end;
    }
    let (data_start, data_len) = data.ok_or(())?;
    Ok(WavFormat {
        sample_rate: sample_rate.ok_or(())?,
        data_start,
        data_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> [u8; 48] {
        let mut bytes = [0; 48];
        bytes[..4].copy_from_slice(b"RIFF");
        bytes[4..8].copy_from_slice(&40_u32.to_le_bytes());
        bytes[8..16].copy_from_slice(b"WAVEfmt ");
        bytes[16..20].copy_from_slice(&16_u32.to_le_bytes());
        bytes[20..24].copy_from_slice(&[1, 0, 2, 0]);
        bytes[24..28].copy_from_slice(&22_050_u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&44_100_u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&[2, 0, 8, 0]);
        bytes[36..40].copy_from_slice(b"data");
        bytes[40..44].copy_from_slice(&4_u32.to_le_bytes());
        bytes
    }

    fn parse_bytes(bytes: &[u8]) -> Result<WavFormat, ()> {
        parse(bytes.len(), |offset, output| {
            let input = bytes.get(offset..).ok_or(())?;
            let count = input.len().min(output.len()).min(3);
            output[..count].copy_from_slice(&input[..count]);
            Ok(count)
        })
    }

    #[test]
    fn reads_partial_io_and_rejects_truncation_and_bad_chunk_sizes() {
        let mut bytes = fixture();
        assert_eq!(
            parse_bytes(&bytes),
            Ok(WavFormat {
                sample_rate: 22_050,
                data_start: 44,
                data_len: 4,
            })
        );
        assert_eq!(parse_bytes(&bytes[..47]), Err(()));
        bytes[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_bytes(&bytes), Err(()));
    }

    #[test]
    fn rejects_unsupported_format_and_out_of_range_rate() {
        let mut bytes = fixture();
        bytes[34] = 16;
        assert_eq!(parse_bytes(&bytes), Err(()));
        let mut bytes = fixture();
        bytes[24..28].copy_from_slice(&96_000_u32.to_le_bytes());
        assert_eq!(parse_bytes(&bytes), Err(()));
        assert_eq!(parse(48, |_, _| Ok(0)), Err(()));
    }
}
