#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Clone, Copy, Debug)]
pub struct ScsiSense {
    pub key: u8,
    pub asc: u8,
    pub ascq: u8,
}

impl ScsiSense {
    pub const fn new(key: u8, asc: u8, ascq: u8) -> Self {
        Self { key, asc, ascq }
    }

    // --- Sense Key constants ---
    pub const KEY_NO_SENSE: u8 = 0x00;
    pub const KEY_RECOVERED_ERROR: u8 = 0x01;
    pub const KEY_NOT_READY: u8 = 0x02;
    pub const KEY_MEDIUM_ERROR: u8 = 0x03;
    pub const KEY_HARDWARE_ERROR: u8 = 0x04;
    pub const KEY_ILLEGAL_REQUEST: u8 = 0x05;
    pub const KEY_UNIT_ATTENTION: u8 = 0x06;
    pub const KEY_DATA_PROTECT: u8 = 0x07;
    pub const KEY_BLANK_CHECK: u8 = 0x08;
    pub const KEY_VENDOR_SPECIFIC: u8 = 0x09;
    pub const KEY_COPY_ABORTED: u8 = 0x0A;
    pub const KEY_ABORTED_COMMAND: u8 = 0x0B;
    pub const KEY_VOLUME_OVERFLOW: u8 = 0x0D;
    pub const KEY_MISCOMPARE: u8 = 0x0E;

    // --- Common ASC/ASCQ pairs (subset of most-used codes) ---
    pub const ASC_NO_ADDITIONAL_SENSE: u8 = 0x00;

    // Medium errors
    pub const ASC_UNRECOVERED_READ_ERROR: u8 = 0x11;
    pub const ASCQ_UNRECOVERED_READ_ERROR: u8 = 0x00;
    pub const ASC_UNRECOVERED_WRITE_ERROR: u8 = 0x03;
    pub const ASCQ_UNRECOVERED_WRITE_ERROR: u8 = 0x01;
    pub const ASC_WRITE_FAULT: u8 = 0x03;
    pub const ASC_ID_CRC_ERROR: u8 = 0x10;

    // Illegal requests
    pub const ASC_INVALID_COMMAND_OPERATION_CODE: u8 = 0x20;
    pub const ASC_LBA_OUT_OF_RANGE: u8 = 0x21;
    pub const ASC_INVALID_FIELD_IN_CDB: u8 = 0x24;

    // Not ready
    pub const ASC_LOGICAL_UNIT_NOT_READY: u8 = 0x04;
    pub const ASC_MEDIUM_NOT_PRESENT: u8 = 0x3A;

    // Unit attention
    pub const ASC_POWER_ON_RESET_OCCURRED: u8 = 0x29;
    pub const ASC_MEDIUM_CHANGED: u8 = 0x28;

    // Data protect
    pub const ASC_WRITE_PROTECTED: u8 = 0x27;

    // Aborted command
    pub const ASC_COMMAND_ABORTED: u8 = 0x47;

    // --- Helpers for common conditions ---
    pub const fn no_sense() -> Self {
        Self::new(Self::KEY_NO_SENSE, Self::ASC_NO_ADDITIONAL_SENSE, 0x00)
    }

    pub const fn invalid_cdb() -> Self {
        Self::new(
            Self::KEY_ILLEGAL_REQUEST,
            Self::ASC_INVALID_FIELD_IN_CDB,
            0x00,
        )
    }

    pub const fn lba_out_of_range() -> Self {
        Self::new(Self::KEY_ILLEGAL_REQUEST, Self::ASC_LBA_OUT_OF_RANGE, 0x00)
    }

    pub const fn medium_not_present() -> Self {
        Self::new(Self::KEY_NOT_READY, Self::ASC_MEDIUM_NOT_PRESENT, 0x00)
    }

    pub const fn unrecovered_read_error() -> Self {
        Self::new(
            Self::KEY_MEDIUM_ERROR,
            Self::ASC_UNRECOVERED_READ_ERROR,
            Self::ASCQ_UNRECOVERED_READ_ERROR,
        )
    }

    pub const fn unrecovered_write_error() -> Self {
        Self::new(
            Self::KEY_MEDIUM_ERROR,
            Self::ASC_UNRECOVERED_WRITE_ERROR,
            Self::ASCQ_UNRECOVERED_WRITE_ERROR,
        )
    }

    pub const fn write_protected() -> Self {
        Self::new(Self::KEY_DATA_PROTECT, Self::ASC_WRITE_PROTECTED, 0x00)
    }
}
