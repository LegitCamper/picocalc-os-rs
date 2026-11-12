//! PTP/MTP container and dataset encoding.
//!
//! Pure byte twiddling, no I/O, no deps, so it is testable on the host:
//! `rustc --test --edition 2024 kernel/src/mtp/codec.rs -o /tmp/mtp_codec && /tmp/mtp_codec`

/// Length of a PTP bulk container header.
pub const HDR: usize = 12;

pub const CT_COMMAND: u16 = 1;
pub const CT_DATA: u16 = 2;
pub const CT_RESPONSE: u16 = 3;

pub const OP_GET_DEVICE_INFO: u16 = 0x1001;
pub const OP_OPEN_SESSION: u16 = 0x1002;
pub const OP_CLOSE_SESSION: u16 = 0x1003;
pub const OP_GET_STORAGE_IDS: u16 = 0x1004;
pub const OP_GET_STORAGE_INFO: u16 = 0x1005;
pub const OP_GET_NUM_OBJECTS: u16 = 0x1006;
pub const OP_GET_OBJECT_HANDLES: u16 = 0x1007;
pub const OP_GET_OBJECT_INFO: u16 = 0x1008;
pub const OP_GET_OBJECT: u16 = 0x1009;
pub const OP_DELETE_OBJECT: u16 = 0x100B;
pub const OP_SEND_OBJECT_INFO: u16 = 0x100C;
pub const OP_SEND_OBJECT: u16 = 0x100D;

pub const RC_OK: u16 = 0x2001;
pub const RC_GENERAL_ERROR: u16 = 0x2002;
pub const RC_INCOMPLETE_TRANSFER: u16 = 0x2007;
pub const RC_SESSION_NOT_OPEN: u16 = 0x2003;
pub const RC_OPERATION_NOT_SUPPORTED: u16 = 0x2005;
pub const RC_INVALID_STORAGE_ID: u16 = 0x2008;
pub const RC_INVALID_OBJECT_HANDLE: u16 = 0x2009;
pub const RC_STORE_NOT_AVAILABLE: u16 = 0x2013;
pub const RC_NO_VALID_OBJECT_INFO: u16 = 0x2015;
pub const RC_INVALID_PARAMETER: u16 = 0x201D;
pub const RC_INVALID_PARENT_OBJECT: u16 = 0x201A;
pub const RC_SESSION_ALREADY_OPEN: u16 = 0x201E;

pub const FMT_UNDEFINED: u16 = 0x3000;
pub const FMT_ASSOCIATION: u16 = 0x3001;

/// Offset of the `filename` field inside an ObjectInfo dataset.
const OBJECT_INFO_NAME_OFFSET: usize = 52;
/// Offset of the `association_type` field inside an ObjectInfo dataset.
const OBJECT_INFO_ASSOC_OFFSET: usize = 42;

/// A PTP bulk container header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// Total container length, header included.
    pub len: u32,
    /// Container type, one of `CT_*`.
    pub kind: u16,
    /// Operation or response code.
    pub code: u16,
    /// Transaction id this container belongs to.
    pub tid: u32,
}

impl Header {
    pub fn parse(buf: &[u8]) -> Option<Self> {
        let raw = buf.get(..HDR)?;
        let len = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        if (len as usize) < HDR {
            return None;
        }
        Some(Self {
            len,
            kind: u16::from_le_bytes([raw[4], raw[5]]),
            code: u16::from_le_bytes([raw[6], raw[7]]),
            tid: u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]),
        })
    }

    pub fn write(&self, buf: &mut [u8]) -> Option<()> {
        let raw = buf.get_mut(..HDR)?;
        raw[..4].copy_from_slice(&self.len.to_le_bytes());
        raw[4..6].copy_from_slice(&self.kind.to_le_bytes());
        raw[6..8].copy_from_slice(&self.code.to_le_bytes());
        raw[8..].copy_from_slice(&self.tid.to_le_bytes());
        Some(())
    }
}

pub struct DataPhase {
    pub remaining: u32,
}

impl DataPhase {
    pub fn start(packet: &[u8], operation: u16, tid: u32) -> Option<(Self, Header)> {
        let header = Header::parse(packet)?;
        if header.kind != CT_DATA || header.code != operation || header.tid != tid {
            return None;
        }
        let received = u32::try_from(packet.len()).ok()?;
        let remaining = header.len.checked_sub(received)?;
        Some((Self { remaining }, header))
    }

    pub fn accept(&mut self, length: usize) -> Option<()> {
        if length == 0 {
            return None;
        }
        self.remaining = self.remaining.checked_sub(u32::try_from(length).ok()?)?;
        Some(())
    }
}

/// Up to five command parameters, missing ones read as 0.
pub fn params(payload: &[u8]) -> [u32; 5] {
    let mut out = [0u32; 5];
    for (i, slot) in out.iter_mut().enumerate() {
        let at = i * 4;
        if let Some(raw) = payload.get(at..at + 4) {
            *slot = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        }
    }
    out
}

/// Little-endian dataset writer over a fixed buffer. Every method returns
/// `None` once the buffer is full, so a truncated dataset is never sent.
pub struct Cursor<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn len(&self) -> usize {
        self.pos
    }

    pub fn bytes(&mut self, src: &[u8]) -> Option<()> {
        self.buf
            .get_mut(self.pos..self.pos + src.len())?
            .copy_from_slice(src);
        self.pos += src.len();
        Some(())
    }

    pub fn u8(&mut self, v: u8) -> Option<()> {
        self.bytes(&[v])
    }

    pub fn u16(&mut self, v: u16) -> Option<()> {
        self.bytes(&v.to_le_bytes())
    }

    pub fn u32(&mut self, v: u32) -> Option<()> {
        self.bytes(&v.to_le_bytes())
    }

    pub fn u64(&mut self, v: u64) -> Option<()> {
        self.bytes(&v.to_le_bytes())
    }

    /// PTP string: `u8` length in UTF-16 units including terminator, then UTF-16LE.
    /// ponytail: non-ASCII becomes `?`; upgrade to full UTF-16 when filenames need it.
    pub fn string(&mut self, s: &str) -> Option<()> {
        if s.is_empty() {
            return self.u8(0);
        }
        let units = s.chars().count().checked_add(1)?;
        if units > u8::MAX as usize {
            return None;
        }
        self.u8(units as u8)?;
        for c in s.chars() {
            let unit = if c.is_ascii() { c as u16 } else { b'?' as u16 };
            self.u16(unit)?;
        }
        self.u16(0)
    }

    pub fn array_u16(&mut self, items: &[u16]) -> Option<()> {
        self.u32(items.len() as u32)?;
        for &i in items {
            self.u16(i)?;
        }
        Some(())
    }

    pub fn array_u32(&mut self, items: impl ExactSizeIterator<Item = u32>) -> Option<()> {
        self.u32(items.len() as u32)?;
        for i in items {
            self.u32(i)?;
        }
        Some(())
    }
}

/// The fields of a host-sent ObjectInfo that this responder honours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewObject {
    /// Announced size of the data component in bytes.
    pub size: u32,
    /// True when the host is creating a directory rather than a file.
    pub is_dir: bool,
    /// Length of the name written into `name_out`.
    pub name_len: usize,
}

/// Parse the ObjectInfo dataset sent before `SendObject`, writing the filename
/// into `name_out` as ASCII.
pub fn parse_object_info(data: &[u8], name_out: &mut [u8]) -> Option<NewObject> {
    let size = u32::from_le_bytes([*data.get(8)?, *data.get(9)?, *data.get(10)?, *data.get(11)?]);
    let format = u16::from_le_bytes([*data.get(4)?, *data.get(5)?]);
    let assoc = u16::from_le_bytes([
        *data.get(OBJECT_INFO_ASSOC_OFFSET)?,
        *data.get(OBJECT_INFO_ASSOC_OFFSET + 1)?,
    ]);

    let units = *data.get(OBJECT_INFO_NAME_OFFSET)? as usize;
    if units < 2 {
        return None;
    }
    let encoded = data.get(OBJECT_INFO_NAME_OFFSET + 1..OBJECT_INFO_NAME_OFFSET + 1 + units * 2)?;
    if encoded[encoded.len() - 2..] != [0, 0] {
        return None;
    }
    let chars = units - 1;
    let mut name_len = 0;
    for i in 0..chars {
        let at = OBJECT_INFO_NAME_OFFSET + 1 + i * 2;
        let unit = u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]);
        if !(0x20..0x7f).contains(&unit) || matches!(unit as u8, b'/' | b'\\') {
            return None;
        }
        *name_out.get_mut(name_len)? = unit as u8;
        name_len += 1;
    }

    if matches!(&name_out[..name_len], b"." | b"..") {
        return None;
    }

    Some(NewObject {
        size,
        is_dir: format == FMT_ASSOCIATION || assoc == 1,
        name_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrips() {
        let hdr = Header {
            len: 24,
            kind: CT_COMMAND,
            code: OP_GET_OBJECT,
            tid: 7,
        };
        let mut buf = [0u8; HDR];
        hdr.write(&mut buf).unwrap();
        assert_eq!(Header::parse(&buf), Some(hdr));
    }

    #[test]
    fn header_parse_rejects_short_length_field() {
        let mut buf = [0u8; HDR];
        buf[0] = 4;
        assert_eq!(Header::parse(&buf), None);
    }

    #[test]
    fn params_zero_fills_missing() {
        assert_eq!(params(&1u32.to_le_bytes()), [1, 0, 0, 0, 0]);
    }

    #[test]
    fn string_is_utf16le_with_terminator() {
        let mut buf = [0u8; 16];
        let mut c = Cursor::new(&mut buf);
        c.string("AB").unwrap();
        assert_eq!(c.len(), 7);
        assert_eq!(&buf[..7], &[3, b'A', 0, b'B', 0, 0, 0]);
    }

    #[test]
    fn empty_string_is_single_zero() {
        let mut buf = [0u8; 4];
        let mut c = Cursor::new(&mut buf);
        c.string("").unwrap();
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn cursor_refuses_to_overflow() {
        let mut buf = [0u8; 3];
        let mut c = Cursor::new(&mut buf);
        assert_eq!(c.u32(1), None);
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn object_info_name_and_size_parse() {
        let mut buf = [0u8; 64];
        buf[8..12].copy_from_slice(&1234u32.to_le_bytes());
        buf[OBJECT_INFO_NAME_OFFSET] = 3; // "AB" + NUL
        buf[OBJECT_INFO_NAME_OFFSET + 1] = b'A';
        buf[OBJECT_INFO_NAME_OFFSET + 3] = b'B';
        let mut name = [0u8; 16];
        let info = parse_object_info(&buf, &mut name).unwrap();
        assert_eq!(info.size, 1234);
        assert_eq!(&name[..info.name_len], b"AB");
        assert!(!info.is_dir);
    }

    #[test]
    fn object_info_detects_directory() {
        let mut buf = [0u8; 64];
        buf[4..6].copy_from_slice(&FMT_ASSOCIATION.to_le_bytes());
        buf[OBJECT_INFO_NAME_OFFSET] = 2;
        buf[OBJECT_INFO_NAME_OFFSET + 1] = b'A';
        let mut name = [0u8; 4];
        assert!(parse_object_info(&buf, &mut name).unwrap().is_dir);
    }

    #[test]
    fn data_phase_accepts_combined_and_split_headers() {
        for size in [0, 1, 51, 52, 53, 64, 500, 512, 1024] {
            for split in [false, true] {
                let mut packet = [0u8; 64];
                Header {
                    len: HDR as u32 + size,
                    kind: CT_DATA,
                    code: OP_SEND_OBJECT,
                    tid: 7,
                }
                .write(&mut packet)
                .unwrap();
                let first = if split {
                    HDR
                } else {
                    (HDR + size as usize).min(64)
                };
                let (mut phase, _) = DataPhase::start(&packet[..first], OP_SEND_OBJECT, 7).unwrap();
                let mut received = first - HDR;
                while phase.remaining > 0 {
                    let length = phase.remaining.min(64) as usize;
                    phase.accept(length).unwrap();
                    received += length;
                }
                assert_eq!(received, size as usize);
            }
        }
    }

    #[test]
    fn data_phase_rejects_wrong_headers_and_lengths() {
        let header = Header {
            len: 64,
            kind: CT_DATA,
            code: OP_SEND_OBJECT,
            tid: 7,
        };
        let mut packet = [0u8; 65];
        header.write(&mut packet).unwrap();
        assert!(DataPhase::start(&packet, OP_SEND_OBJECT, 7).is_none());
        assert!(DataPhase::start(&packet[..12], OP_SEND_OBJECT_INFO, 7).is_none());
        assert!(DataPhase::start(&packet[..12], OP_SEND_OBJECT, 8).is_none());
        assert!(DataPhase::start(&packet[..11], OP_SEND_OBJECT, 7).is_none());
        let (mut phase, _) = DataPhase::start(&packet[..12], OP_SEND_OBJECT, 7).unwrap();
        assert!(phase.accept(0).is_none());
        assert!(phase.accept(53).is_none());
        assert_eq!(phase.remaining, 52);
        Header {
            kind: CT_COMMAND,
            ..header
        }
        .write(&mut packet)
        .unwrap();
        assert!(DataPhase::start(&packet[..12], OP_SEND_OBJECT, 7).is_none());
    }

    #[test]
    fn object_info_rejects_unsafe_or_lossy_names() {
        for name in ["", ".", "..", "a/b", "a\\b", "\0", "\u{e9}"] {
            let mut buf = [0u8; 100];
            Cursor::new(&mut buf[OBJECT_INFO_NAME_OFFSET..])
                .string(name)
                .unwrap();
            if name == "\u{e9}" {
                buf[OBJECT_INFO_NAME_OFFSET + 1] = 0xe9;
            }
            assert!(parse_object_info(&buf, &mut [0; 64]).is_none(), "{name:?}");
        }
    }

    #[test]
    fn object_info_requires_complete_terminated_name() {
        let mut buf = [0u8; 64];
        Cursor::new(&mut buf[OBJECT_INFO_NAME_OFFSET..])
            .string("AB")
            .unwrap();
        assert!(parse_object_info(&buf[..58], &mut [0; 64]).is_none());
        buf[57] = 1;
        assert!(parse_object_info(&buf, &mut [0; 64]).is_none());
    }
}
