use num_enum::TryFromPrimitive;

/// THE CODE BELOW ORIGINATES FROM: https://github.com/apohrebniak/usbd-storage/blob/master/usbd-storage/src/subclass/scsi.rs

/// SCSI device subclass code
pub const SUBCLASS_SCSI: u8 = 0x06; // SCSI Transparent command set

/* SCSI codes */

/* SPC */
const TEST_UNIT_READY: u8 = 0x00;
const REQUEST_SENSE: u8 = 0x03;
const INQUIRY: u8 = 0x12;
const MODE_SENSE_6: u8 = 0x1A;
const MODE_SENSE_10: u8 = 0x5A;

/* SBC */
const READ_10: u8 = 0x28;
const READ_16: u8 = 0x88;
const READ_CAPACITY_10: u8 = 0x25;
const READ_CAPACITY_16: u8 = 0x9E;
const WRITE_10: u8 = 0x2A;

/* MMC */
const READ_FORMAT_CAPACITIES: u8 = 0x23;

/// SCSI command
///
/// Refer to specifications (SPC,SAM,SBC,MMC,etc.)
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[non_exhaustive]
pub enum ScsiCommand {
    Unknown,

    /* SPC */
    Inquiry {
        evpd: bool,
        page_code: u8,
        alloc_len: u16,
    },
    TestUnitReady,
    RequestSense {
        desc: bool,
        alloc_len: u8,
    },
    ModeSense6 {
        dbd: bool,
        page_control: PageControl,
        page_code: u8,
        subpage_code: u8,
        alloc_len: u8,
    },
    ModeSense10 {
        dbd: bool,
        page_control: PageControl,
        page_code: u8,
        subpage_code: u8,
        alloc_len: u16,
    },

    /* SBC */
    ReadCapacity10,
    ReadCapacity16 {
        alloc_len: u32,
    },
    Read {
        lba: u64,
        len: u64,
    },
    Write {
        lba: u64,
        len: u64,
    },

    /* MMC */
    ReadFormatCapacities {
        alloc_len: u16,
    },
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, TryFromPrimitive)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum PageControl {
    CurrentValues = 0b00,
    ChangeableValues = 0b01,
    DefaultValues = 0b10,
    SavedValues = 0b11,
}

#[allow(dead_code)]
pub fn parse_cb(cb: &[u8]) -> ScsiCommand {
    match cb[0] {
        TEST_UNIT_READY => ScsiCommand::TestUnitReady,
        INQUIRY => ScsiCommand::Inquiry {
            evpd: (cb[1] & 0b00000001) != 0,
            page_code: cb[2],
            alloc_len: u16::from_be_bytes([cb[3], cb[4]]),
        },
        REQUEST_SENSE => ScsiCommand::RequestSense {
            desc: (cb[1] & 0b00000001) != 0,
            alloc_len: cb[4],
        },
        READ_CAPACITY_10 => ScsiCommand::ReadCapacity10,
        READ_CAPACITY_16 => ScsiCommand::ReadCapacity16 {
            alloc_len: u32::from_be_bytes([cb[10], cb[11], cb[12], cb[13]]),
        },
        READ_10 => ScsiCommand::Read {
            lba: u32::from_be_bytes([cb[2], cb[3], cb[4], cb[5]]) as u64,
            len: u16::from_be_bytes([cb[7], cb[8]]) as u64,
        },
        READ_16 => ScsiCommand::Read {
            lba: u64::from_be_bytes((&cb[2..10]).try_into().unwrap()),
            len: u32::from_be_bytes((&cb[10..14]).try_into().unwrap()) as u64,
        },
        WRITE_10 => ScsiCommand::Write {
            lba: u32::from_be_bytes([cb[2], cb[3], cb[4], cb[5]]) as u64,
            len: u16::from_be_bytes([cb[7], cb[8]]) as u64,
        },
        MODE_SENSE_6 => ScsiCommand::ModeSense6 {
            dbd: (cb[1] & 0b00001000) != 0,
            page_control: PageControl::try_from_primitive(cb[2] >> 6).unwrap(),
            page_code: cb[2] & 0b00111111,
            subpage_code: cb[3],
            alloc_len: cb[4],
        },
        MODE_SENSE_10 => ScsiCommand::ModeSense10 {
            dbd: (cb[1] & 0b00001000) != 0,
            page_control: PageControl::try_from_primitive(cb[2] >> 6).unwrap(),
            page_code: cb[2] & 0b00111111,
            subpage_code: cb[3],
            alloc_len: u16::from_be_bytes([cb[7], cb[8]]),
        },
        READ_FORMAT_CAPACITIES => ScsiCommand::ReadFormatCapacities {
            alloc_len: u16::from_be_bytes([cb[7], cb[8]]),
        },
        _ => ScsiCommand::Unknown,
    }
}
