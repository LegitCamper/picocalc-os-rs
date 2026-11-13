use core::ops::Range;

pub const HEADER_SIZE: usize = 52;
pub const PROGRAM_HEADER_SIZE: usize = 32;
pub const SECTION_HEADER_SIZE: usize = 40;
pub const SYMBOL_SIZE: usize = 16;
pub const REL_SIZE: usize = 8;
pub const MAX_PROGRAM_HEADERS: usize = 32;
pub const MAX_RELOCATIONS: u32 = 8192;
pub const MAX_SYMBOLS: u32 = 8192;
pub const SRAM_IMAGE_SIZE: usize = 250 * 1024;
// ponytail: match app linker RAM ceiling; larger apps need a larger linker region and budget.
// PSRAM boards allow larger PSRAM-compatible images such as Doom in the XIP window.
#[cfg(not(feature = "psram"))]
pub const MAX_IMAGE_SIZE: usize = SRAM_IMAGE_SIZE;
#[cfg(feature = "psram")]
pub const MAX_IMAGE_SIZE: usize = 640 * 1024;

#[cfg(any(feature = "psram", test))]
pub const fn needs_psram(size: usize) -> bool {
    size > SRAM_IMAGE_SIZE
}
pub const PT_LOAD: u32 = 1;
pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const SHT_SYMTAB: u32 = 2;
pub const SHT_STRTAB: u32 = 3;
pub const SHT_RELA: u32 = 4;
pub const SHT_REL: u32 = 9;
pub const SHT_DYNSYM: u32 = 11;
pub const SHF_WRITE: u32 = 1;
pub const SHF_ALLOC: u32 = 2;
pub const R_ARM_RELATIVE: u32 = 23;

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    WrongMachine,
    InvalidElf,
    FailedToReadFile,
    ElfIsNotPie,
    UnknownRelocationType,
    SyscallTableNotFound,
    SyscallTableSizeMismatch,
    TooLarge,
    OutOfMemory,
}

pub struct Header {
    pub entry: u32,
    pub program_table: u32,
    pub section_table: u32,
    pub program_count: u16,
    pub section_count: u16,
}

impl Header {
    pub fn parse(bytes: &[u8; HEADER_SIZE], file_size: u32) -> Result<Self, LoadError> {
        if bytes[..7] != *b"\x7fELF\x01\x01\x01"
            || word(bytes, 20) != 1
            || half(bytes, 40) != HEADER_SIZE as u16
            || half(bytes, 42) != PROGRAM_HEADER_SIZE as u16
            || half(bytes, 46) != SECTION_HEADER_SIZE as u16
        {
            return Err(LoadError::InvalidElf);
        }
        if half(bytes, 16) != 3 {
            return Err(LoadError::ElfIsNotPie);
        }
        if half(bytes, 18) != 40 {
            return Err(LoadError::WrongMachine);
        }
        let header = Self {
            entry: word(bytes, 24),
            program_table: word(bytes, 28),
            section_table: word(bytes, 32),
            program_count: half(bytes, 44),
            section_count: half(bytes, 48),
        };
        if header.program_count == 0
            || header.program_count as usize > MAX_PROGRAM_HEADERS
            || header.section_count == 0
            || header.section_count > 256
            || half(bytes, 50) >= header.section_count
            || header.program_table < HEADER_SIZE as u32
            || header.section_table < HEADER_SIZE as u32
        {
            return Err(LoadError::InvalidElf);
        }
        checked_range(
            header.program_table,
            u32::from(header.program_count) * PROGRAM_HEADER_SIZE as u32,
            file_size,
        )?;
        checked_range(
            header.section_table,
            u32::from(header.section_count) * SECTION_HEADER_SIZE as u32,
            file_size,
        )?;
        Ok(header)
    }

    pub fn program_offset(&self, index: u16) -> Result<u32, LoadError> {
        table_offset(
            self.program_table,
            index,
            self.program_count,
            PROGRAM_HEADER_SIZE,
        )
    }

    pub fn section_offset(&self, index: u16) -> Result<u32, LoadError> {
        table_offset(
            self.section_table,
            index,
            self.section_count,
            SECTION_HEADER_SIZE,
        )
    }
}

#[derive(Clone, Copy)]
pub struct ProgramHeader {
    pub kind: u32,
    pub offset: u32,
    pub address: u32,
    pub file_size: u32,
    pub memory_size: u32,
    pub flags: u32,
    pub alignment: u32,
}

impl ProgramHeader {
    pub const EMPTY: Self = Self {
        kind: 0,
        offset: 0,
        address: 0,
        file_size: 0,
        memory_size: 0,
        flags: 0,
        alignment: 0,
    };

    pub fn parse(bytes: &[u8; PROGRAM_HEADER_SIZE], file_size: u32) -> Result<Self, LoadError> {
        let segment = Self {
            kind: word(bytes, 0),
            offset: word(bytes, 4),
            address: word(bytes, 8),
            file_size: word(bytes, 16),
            memory_size: word(bytes, 20),
            flags: word(bytes, 24),
            alignment: word(bytes, 28),
        };
        if segment.kind == PT_LOAD {
            if segment.file_size > segment.memory_size
                || segment.address.checked_add(segment.memory_size).is_none()
                || segment.alignment > 65536
                || (segment.alignment > 1
                    && (!segment.alignment.is_power_of_two()
                        || segment.offset % segment.alignment
                            != segment.address % segment.alignment))
            {
                return Err(LoadError::InvalidElf);
            }
            checked_range(segment.offset, segment.file_size, file_size)?;
        }
        Ok(segment)
    }
}

pub struct Image<'a> {
    pub origin: u32,
    pub size: usize,
    pub alignment: usize,
    segments: &'a [ProgramHeader],
}

impl<'a> Image<'a> {
    pub fn new(segments: &'a [ProgramHeader], entry: u32) -> Result<Self, LoadError> {
        let mut origin = u32::MAX;
        let mut end = 0;
        let mut alignment = 4;
        for (index, segment) in segments.iter().enumerate() {
            let segment_end = segment
                .address
                .checked_add(segment.memory_size)
                .ok_or(LoadError::InvalidElf)?;
            for other in &segments[..index] {
                if segment.address < other.address + other.memory_size
                    && other.address < segment_end
                {
                    return Err(LoadError::InvalidElf);
                }
            }
            origin = origin.min(segment.address);
            end = end.max(segment_end);
            alignment = alignment.max(segment.alignment);
        }
        if segments.is_empty() || !alignment.is_power_of_two() || entry & 1 == 0 {
            return Err(LoadError::InvalidElf);
        }
        origin &= !(alignment - 1);
        let size = end.checked_sub(origin).ok_or(LoadError::InvalidElf)? as usize;
        if size == 0 || size > MAX_IMAGE_SIZE {
            return Err(LoadError::TooLarge);
        }
        let image = Self {
            origin,
            size,
            alignment: alignment as usize,
            segments,
        };
        image.range(entry & !1, 2, PF_X)?;
        if !segments.iter().any(|segment| {
            segment.flags & PF_X != 0
                && contained_range(entry & !1, 2, segment.address, segment.file_size).is_ok()
        }) {
            return Err(LoadError::InvalidElf);
        }
        Ok(image)
    }

    pub fn range(&self, address: u32, size: u32, flags: u32) -> Result<Range<usize>, LoadError> {
        if !self.segments.iter().any(|segment| {
            segment.flags & flags == flags
                && contained_range(address, size, segment.address, segment.memory_size).is_ok()
        }) {
            return Err(LoadError::InvalidElf);
        }
        contained_range(address, size, self.origin, self.size as u32)
    }

    pub fn relocation_range(
        &self,
        bytes: &[u8; REL_SIZE],
        target: Option<&SectionHeader>,
    ) -> Result<Option<Range<usize>>, LoadError> {
        let (address, info) = relocation(bytes);
        if info == 0 {
            return Ok(None);
        }
        if info != R_ARM_RELATIVE {
            return Err(LoadError::UnknownRelocationType);
        }
        if address % 4 != 0 {
            return Err(LoadError::InvalidElf);
        }
        if let Some(target) = target {
            contained_range(address, 4, target.address, target.size)?;
        }
        self.range(address, 4, 0).map(Some)
    }

    pub fn syscall_range(
        &self,
        symbol: &Symbol,
        section: &SectionHeader,
        count: usize,
    ) -> Result<Range<usize>, LoadError> {
        if count.checked_mul(4) != Some(symbol.size as usize) {
            return Err(LoadError::SyscallTableSizeMismatch);
        }
        if symbol.kind != 1
            || symbol.address % 4 != 0
            || section.flags & (SHF_ALLOC | SHF_WRITE) != SHF_ALLOC | SHF_WRITE
        {
            return Err(LoadError::InvalidElf);
        }
        contained_range(symbol.address, symbol.size, section.address, section.size)?;
        self.range(symbol.address, symbol.size, PF_W)
    }
}

#[derive(Clone, Copy)]
pub struct SectionHeader {
    pub kind: u32,
    pub flags: u32,
    pub address: u32,
    pub offset: u32,
    pub size: u32,
    pub link: u32,
    pub info: u32,
}

impl SectionHeader {
    pub fn parse(
        bytes: &[u8; SECTION_HEADER_SIZE],
        file_size: u32,
        section_count: u16,
    ) -> Result<Self, LoadError> {
        let section = Self {
            kind: word(bytes, 4),
            flags: word(bytes, 8),
            address: word(bytes, 12),
            offset: word(bytes, 16),
            size: word(bytes, 20),
            link: word(bytes, 24),
            info: word(bytes, 28),
        };
        let alignment = word(bytes, 32);
        if alignment > 1 && (!alignment.is_power_of_two() || section.address % alignment != 0) {
            return Err(LoadError::InvalidElf);
        }
        if section.kind != 0 && section.kind != 8 {
            checked_range(section.offset, section.size, file_size)?;
        }
        let stride = match section.kind {
            SHT_REL => REL_SIZE,
            SHT_RELA => 12,
            SHT_SYMTAB | SHT_DYNSYM => SYMBOL_SIZE,
            _ => return Ok(section),
        } as u32;
        if word(bytes, 36) != stride
            || section.size % stride != 0
            || section.link == 0
            || section.link >= u32::from(section_count)
            || (matches!(section.kind, SHT_REL | SHT_RELA)
                && section.info >= u32::from(section_count))
            || (matches!(section.kind, SHT_SYMTAB | SHT_DYNSYM)
                && section.info > section.size / stride)
        {
            return Err(LoadError::InvalidElf);
        }
        Ok(section)
    }
}

pub struct Symbol {
    pub name: u32,
    pub address: u32,
    pub size: u32,
    pub kind: u8,
    pub section: u16,
}

impl Symbol {
    pub fn parse(bytes: &[u8; SYMBOL_SIZE]) -> Self {
        Self {
            name: word(bytes, 0),
            address: word(bytes, 4),
            size: word(bytes, 8),
            kind: bytes[12] & 0xf,
            section: half(bytes, 14),
        }
    }

    pub fn validate_section(&self, count: u16) -> Result<(), LoadError> {
        if self.section == 0 || self.section >= count {
            Err(LoadError::InvalidElf)
        } else {
            Ok(())
        }
    }
}

pub fn relocation(bytes: &[u8; REL_SIZE]) -> (u32, u32) {
    (word(bytes, 0), word(bytes, 4))
}

pub fn relative_value(base: u32, origin: u32, addend: u32) -> u32 {
    base.wrapping_sub(origin).wrapping_add(addend)
}

pub async fn read_exact_with(
    mut bytes: &mut [u8],
    mut read: impl AsyncFnMut(&mut [u8]) -> Result<usize, LoadError>,
) -> Result<(), LoadError> {
    while !bytes.is_empty() {
        let chunk_len = bytes.len().min(512);
        let count = read(&mut bytes[..chunk_len]).await?;
        if count == 0 || count > chunk_len {
            return Err(LoadError::FailedToReadFile);
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}

pub fn checked_range(offset: u32, size: u32, limit: u32) -> Result<Range<usize>, LoadError> {
    let end = offset.checked_add(size).ok_or(LoadError::InvalidElf)?;
    if end > limit {
        return Err(LoadError::InvalidElf);
    }
    Ok(offset as usize..end as usize)
}

pub fn contained_range(
    address: u32,
    size: u32,
    origin: u32,
    length: u32,
) -> Result<Range<usize>, LoadError> {
    let offset = address.checked_sub(origin).ok_or(LoadError::InvalidElf)?;
    address.checked_add(size).ok_or(LoadError::InvalidElf)?;
    checked_range(offset, size, length)
}

fn table_offset(base: u32, index: u16, count: u16, stride: usize) -> Result<u32, LoadError> {
    if index >= count {
        return Err(LoadError::InvalidElf);
    }
    base.checked_add(u32::from(index) * stride as u32)
        .ok_or(LoadError::InvalidElf)
}

fn half(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires just userapps; run by just check after rebuilding"]
    fn rebuilt_apps_match_loader_contract() {
        for app in [
            "calculator",
            "snake",
            "gallery",
            "gif",
            "wav_player",
            "gboy",
        ] {
            let bytes = std::fs::read(format!(
                "target/thumbv8m.main-none-eabihf/release-binary/{app}"
            ))
            .unwrap();
            let size = u32::try_from(bytes.len()).unwrap();
            let header = Header::parse(bytes[..HEADER_SIZE].try_into().unwrap(), size).unwrap();
            let segments: Vec<_> = (0..header.program_count)
                .map(|index| {
                    let offset = header.program_offset(index).unwrap() as usize;
                    ProgramHeader::parse(
                        bytes[offset..offset + PROGRAM_HEADER_SIZE]
                            .try_into()
                            .unwrap(),
                        size,
                    )
                    .unwrap()
                })
                .filter(|segment| segment.kind == PT_LOAD && segment.memory_size != 0)
                .collect();
            let image = Image::new(&segments, header.entry).unwrap();
            let section = |index| {
                let offset = header.section_offset(index).unwrap() as usize;
                SectionHeader::parse(
                    bytes[offset..offset + SECTION_HEADER_SIZE]
                        .try_into()
                        .unwrap(),
                    size,
                    header.section_count,
                )
                .unwrap()
            };
            let mut table = None;
            for index in 1..header.section_count {
                let current = section(index);
                if current.flags & SHF_ALLOC != 0 && current.size != 0 {
                    image.range(current.address, current.size, 0).unwrap();
                }
                match current.kind {
                    SHT_REL => {
                        let symbols = section(current.link as u16);
                        assert!(matches!(symbols.kind, SHT_SYMTAB | SHT_DYNSYM));
                        let target = (current.info != 0).then(|| section(current.info as u16));
                        let range = checked_range(current.offset, current.size, size).unwrap();
                        for relocation in bytes[range].chunks_exact(REL_SIZE) {
                            image
                                .relocation_range(relocation.try_into().unwrap(), target.as_ref())
                                .unwrap();
                        }
                    }
                    SHT_RELA => panic!("unsupported relocation in {app}"),
                    SHT_SYMTAB | SHT_DYNSYM => {
                        let strings = section(current.link as u16);
                        assert_eq!(strings.kind, SHT_STRTAB);
                        let strings =
                            &bytes[checked_range(strings.offset, strings.size, size).unwrap()];
                        let range = checked_range(current.offset, current.size, size).unwrap();
                        for raw in bytes[range].chunks_exact(SYMBOL_SIZE) {
                            let symbol = Symbol::parse(raw.try_into().unwrap());
                            assert!((symbol.name as usize) < strings.len());
                            if strings[symbol.name as usize..].starts_with(b"SYS_CALL_TABLE\0") {
                                symbol.validate_section(header.section_count).unwrap();
                                image
                                    .syscall_range(&symbol, &section(symbol.section), 18)
                                    .unwrap();
                                assert!(table.is_none_or(|address| address == symbol.address));
                                table = Some(symbol.address);
                            }
                        }
                    }
                    _ => (),
                }
            }
            assert!(table.is_some(), "{app} has no syscall table");
        }
    }

    fn header_bytes() -> [u8; HEADER_SIZE] {
        let mut bytes = [0; HEADER_SIZE];
        bytes[..7].copy_from_slice(b"\x7fELF\x01\x01\x01");
        for (offset, value) in [
            (16, 3u16),
            (18, 40),
            (40, 52),
            (42, 32),
            (44, 1),
            (46, 40),
            (48, 2),
        ] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [(20, 1u32), (24, 0x1001), (28, 52), (32, 84)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn segment(address: u32, flags: u32) -> ProgramHeader {
        ProgramHeader {
            kind: PT_LOAD,
            address,
            file_size: 16,
            memory_size: 32,
            flags,
            alignment: 4,
            ..ProgramHeader::EMPTY
        }
    }

    #[test]
    fn headers_reject_wrong_identity_sizes_counts_and_truncation() {
        let valid = header_bytes();
        assert!(Header::parse(&valid, 164).is_ok());
        assert!(Header::parse(&valid, 163).is_err());
        for offset in [0, 4, 5, 6, 20, 40, 42, 44, 46, 48] {
            let mut invalid = valid;
            invalid[offset] = 0;
            assert!(Header::parse(&invalid, 164).is_err(), "offset {offset}");
        }
        let header = Header::parse(&valid, 164).unwrap();
        assert_eq!(header.section_offset(1), Ok(124));
        assert!(header.section_offset(2).is_err());
        assert!(header.program_offset(1).is_err());
    }

    #[test]
    fn headers_reject_overflowing_tables_and_extended_numbering() {
        for (offset, value) in [(28, u32::MAX), (32, u32::MAX), (20, 0x101)] {
            let mut bytes = header_bytes();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(Header::parse(&bytes, u32::MAX).is_err());
        }
        for (offset, value) in [(44, 33u16), (44, u16::MAX), (48, 257), (50, 2)] {
            let mut bytes = header_bytes();
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            assert!(Header::parse(&bytes, u32::MAX).is_err());
        }
    }

    #[test]
    fn program_validation_skips_non_load_and_rejects_bad_extents() {
        let mut bytes = [0; PROGRAM_HEADER_SIZE];
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[20..24].copy_from_slice(&32u32.to_le_bytes());
        assert!(ProgramHeader::parse(&bytes, 0).is_ok());
        bytes[0..4].copy_from_slice(&PT_LOAD.to_le_bytes());
        assert!(ProgramHeader::parse(&bytes, 32).is_err());
        bytes[8..12].copy_from_slice(&0u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&33u32.to_le_bytes());
        assert!(ProgramHeader::parse(&bytes, 33).is_err());
        bytes[16..20].copy_from_slice(&16u32.to_le_bytes());
        bytes[28..32].copy_from_slice(&3u32.to_le_bytes());
        assert!(ProgramHeader::parse(&bytes, 32).is_err());
        bytes[28..32].copy_from_slice(&4u32.to_le_bytes());
        assert!(ProgramHeader::parse(&bytes, 15).is_err());
        assert!(ProgramHeader::parse(&bytes, 16).is_ok());
    }

    #[test]
    fn image_requires_thumb_file_backed_entry_and_nonoverlapping_loads() {
        assert!(Image::new(&[], 1).is_err());
        let segments = [segment(0x1000, PF_X), segment(0x1100, PF_W)];
        let image = Image::new(&segments, 0x1001).unwrap();
        assert_eq!(image.range(0x1100, 4, PF_W), Ok(0x100..0x104));
        for entry in [0x1000, 0x1011, 0x1081, 0x1101, u32::MAX] {
            assert!(Image::new(&segments, entry).is_err());
        }
        assert!(image.range(0x1080, 4, 0).is_err());
        assert!(image.range(0x101f, 4, 0).is_err());
        assert!(image.range(0x1000, 4, PF_W).is_err());
        assert!(Image::new(&[segment(0x1000, PF_X), segment(0x1010, PF_W)], 0x1001).is_err());
    }

    #[test]
    fn image_alignment_preserves_load_bias_and_limits_span() {
        let mut load = segment(0x1020, PF_X);
        load.alignment = 4096;
        let segments = [load];
        let image = Image::new(&segments, 0x1021).unwrap();
        assert_eq!((image.origin, image.alignment), (0x1000, 4096));
        load.memory_size = MAX_IMAGE_SIZE as u32;
        assert!(Image::new(&[load], 0x1021).is_err());
    }

    #[test]
    fn sections_reject_zero_stride_remainders_and_bad_links() {
        let mut bytes = [0; SECTION_HEADER_SIZE];
        bytes[4..8].copy_from_slice(&SHT_REL.to_le_bytes());
        bytes[20..24].copy_from_slice(&8u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&1u32.to_le_bytes());
        assert!(SectionHeader::parse(&bytes, 8, 3).is_err());
        bytes[36..40].copy_from_slice(&8u32.to_le_bytes());
        assert!(SectionHeader::parse(&bytes, 8, 3).is_ok());
        bytes[20..24].copy_from_slice(&9u32.to_le_bytes());
        assert!(SectionHeader::parse(&bytes, 9, 3).is_err());
        bytes[20..24].copy_from_slice(&8u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&3u32.to_le_bytes());
        assert!(SectionHeader::parse(&bytes, 8, 3).is_err());
    }

    #[test]
    fn ordinary_images_stay_in_sram_for_exclusive_atomics() {
        assert!(!needs_psram(1));
        assert!(!needs_psram(SRAM_IMAGE_SIZE));
        assert!(needs_psram(SRAM_IMAGE_SIZE + 1));
        assert!(needs_psram(520_176));
    }

    #[test]
    fn exact_reads_retry_partial_data_and_stop_on_eof_or_error() {
        fn ready<Result>(future: impl core::future::Future<Output = Result>) -> Result {
            let mut future = core::pin::pin!(future);
            let mut context = core::task::Context::from_waker(core::task::Waker::noop());
            match future.as_mut().poll(&mut context) {
                core::task::Poll::Ready(result) => result,
                core::task::Poll::Pending => panic!("read unexpectedly yielded"),
            }
        }

        let mut bytes = [0; 4];
        let mut next = 0;
        ready(read_exact_with(&mut bytes, async |remaining| {
            next += 1;
            remaining[0] = next;
            Ok(1)
        }))
        .unwrap();
        assert_eq!(bytes, [1, 2, 3, 4]);
        assert_eq!(
            ready(read_exact_with(&mut bytes, async |_| Ok(0))),
            Err(LoadError::FailedToReadFile)
        );
        assert_eq!(
            ready(read_exact_with(&mut bytes, async |_| Ok(5))),
            Err(LoadError::FailedToReadFile)
        );
        assert_eq!(
            ready(read_exact_with(&mut bytes, async |_| {
                Err(LoadError::FailedToReadFile)
            })),
            Err(LoadError::FailedToReadFile)
        );
        assert!(ready(read_exact_with(&mut [], async |_| panic!("empty read"))).is_ok());
    }

    #[test]
    fn exact_reads_bound_io_and_yield_between_chunks() {
        use core::{
            cell::Cell,
            future::{Future, poll_fn},
            task::{Context, Poll, Waker},
        };

        let mut bytes = [0; 1025];
        let reads = Cell::new(0);
        let mut context = Context::from_waker(Waker::noop());
        {
            let future = read_exact_with(&mut bytes, async |chunk| {
                let mut yielded = false;
                poll_fn(|context| {
                    if yielded {
                        Poll::Ready(())
                    } else {
                        yielded = true;
                        context.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await;
                assert!(chunk.len() <= 512);
                reads.set(reads.get() + 1);
                chunk.fill(0x5a);
                Ok(chunk.len())
            });
            let mut future = core::pin::pin!(future);
            for completed in 0..3 {
                assert_eq!(future.as_mut().poll(&mut context), Poll::Pending);
                assert_eq!(reads.get(), completed);
            }
            assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(Ok(())));
        }
        assert_eq!(reads.get(), 3);
        assert_eq!(bytes, [0x5a; 1025]);
    }

    #[test]
    fn ranges_and_relocations_do_not_wrap_bounds() {
        assert!(checked_range(u32::MAX, 1, u32::MAX).is_err());
        assert!(contained_range(0xfff, 4, 0x1000, 32).is_err());
        assert!(contained_range(0x101f, 4, 0x1000, 32).is_err());
        assert_eq!(relative_value(0x2000_1000, 0x1000, 0x1020), 0x2000_1020);
        assert_eq!(relative_value(0, 0x1000, 0x1020), 0x20);
        let mut bytes = [0; REL_SIZE];
        bytes[..4].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[4..].copy_from_slice(&R_ARM_RELATIVE.to_le_bytes());
        assert_eq!(relocation(&bytes), (0x1000, R_ARM_RELATIVE));
    }

    #[test]
    fn relocation_writes_reject_unknown_symbols_types_unaligned_and_unloaded_targets() {
        let segments = [segment(0x1000, PF_X)];
        let image = Image::new(&segments, 0x1001).unwrap();
        let mut bytes = [0; REL_SIZE];
        assert_eq!(image.relocation_range(&bytes, None), Ok(None));
        bytes[..4].copy_from_slice(&0x1000u32.to_le_bytes());
        bytes[4..].copy_from_slice(&R_ARM_RELATIVE.to_le_bytes());
        assert_eq!(image.relocation_range(&bytes, None), Ok(Some(0..4)));
        for info in [2u32, R_ARM_RELATIVE | 0x100, u32::MAX] {
            bytes[4..].copy_from_slice(&info.to_le_bytes());
            assert_eq!(
                image.relocation_range(&bytes, None),
                Err(LoadError::UnknownRelocationType)
            );
        }
        bytes[4..].copy_from_slice(&R_ARM_RELATIVE.to_le_bytes());
        for address in [0xffcu32, 0x1001, 0x1020, u32::MAX] {
            bytes[..4].copy_from_slice(&address.to_le_bytes());
            assert_eq!(
                image.relocation_range(&bytes, None),
                Err(LoadError::InvalidElf)
            );
        }
        bytes[..4].copy_from_slice(&0x1000u32.to_le_bytes());
        let target = SectionHeader {
            kind: 1,
            flags: SHF_ALLOC,
            address: 0x1010,
            offset: 0,
            size: 4,
            link: 0,
            info: 0,
        };
        assert_eq!(
            image.relocation_range(&bytes, Some(&target)),
            Err(LoadError::InvalidElf)
        );
    }

    #[test]
    fn syscall_table_requires_exact_size_writable_object_and_section_bounds() {
        let segments = [segment(0x1000, PF_X), segment(0x1100, PF_W)];
        let image = Image::new(&segments, 0x1001).unwrap();
        let mut symbol = Symbol {
            name: 0_u32,
            address: 0x1100,
            size: 12,
            kind: 1,
            section: 2,
        };
        let mut section = SectionHeader {
            kind: 8,
            flags: SHF_ALLOC | SHF_WRITE,
            address: 0x1100,
            offset: 0,
            size: 16,
            link: 0,
            info: 0,
        };
        assert_eq!(image.syscall_range(&symbol, &section, 3), Ok(0x100..0x10c));
        assert_eq!(
            image.syscall_range(&symbol, &section, 4),
            Err(LoadError::SyscallTableSizeMismatch)
        );
        assert_eq!(
            image.syscall_range(&symbol, &section, usize::MAX),
            Err(LoadError::SyscallTableSizeMismatch)
        );
        for index in [0, 3, u16::MAX] {
            symbol.section = index;
            assert!(symbol.validate_section(3).is_err());
        }
        symbol.section = 2;
        assert!(symbol.validate_section(3).is_ok());
        symbol.kind = 2;
        assert!(image.syscall_range(&symbol, &section, 3).is_err());
        symbol.kind = 1;
        symbol.address += 1;
        assert!(image.syscall_range(&symbol, &section, 3).is_err());
        symbol.address = 0x1100;
        section.size = 8;
        assert!(image.syscall_range(&symbol, &section, 3).is_err());
        section.size = 16;
        section.flags = SHF_ALLOC;
        assert!(image.syscall_range(&symbol, &section, 3).is_err());
        section.flags |= SHF_WRITE;
        symbol.address = 0x1000;
        section.address = 0x1000;
        assert!(image.syscall_range(&symbol, &section, 3).is_err());
    }

    #[test]
    fn symbols_decode_without_alignment_or_utf8_assumptions() {
        let mut bytes = [0; SYMBOL_SIZE];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&0x1004u32.to_le_bytes());
        bytes[8..12].copy_from_slice(&72u32.to_le_bytes());
        bytes[12] = 0x11;
        bytes[14..16].copy_from_slice(&2u16.to_le_bytes());
        let symbol = Symbol::parse(&bytes);
        assert_eq!(
            (
                symbol.name,
                symbol.address,
                symbol.size,
                symbol.kind,
                symbol.section
            ),
            (3, 0x1004, 72, 1, 2)
        );
    }
}
