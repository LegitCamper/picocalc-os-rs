use embassy_usb::driver::{Driver, EndpointIn, EndpointOut};
use embassy_usb::types::StringIndex;
use embassy_usb::{Builder, Config};

mod scsi_types;
use scsi_types::*;

use crate::storage::SdCard;

pub struct MassStorageClass<'d, 'c, D: Driver<'d>> {
    sdcard: SdCard<'c>,
    bulk_out: D::EndpointOut,
    bulk_in: D::EndpointIn,
}

impl<'d, 'c, D: Driver<'d>> MassStorageClass<'d, 'c, D> {
    pub fn new(builder: &mut Builder<'d, D>, sdcard: SdCard<'c>) -> Self {
        let mut function = builder.function(0x08, 0x06, 0x50); // Mass Storage class
        let mut interface = function.interface();
        let mut alt = interface.alt_setting(0x08, 0x06, 0x50, None);

        let bulk_out = alt.endpoint_bulk_out(64);
        let bulk_in = alt.endpoint_bulk_in(64);

        Self {
            bulk_out,
            bulk_in,
            sdcard,
        }
    }

    pub async fn poll(&mut self) {
        let mut cbw_buf = [0u8; 31];
        if let Ok(n) = self.bulk_out.read(&mut cbw_buf).await {
            if let Some(cbw) = CommandBlockWrapper::parse(&cbw_buf[..n]) {
                self.handle_command(&cbw.CBWCB).await;
            }
        }
    }

    async fn handle_command(&self, cbw: &[u8]) {
        match parse_cb(cbw) {
            ScsiCommand::Unknown => {
                #[cfg(feature = "defmt")]
                defmt::info!("Got unexpected scsi command: {}", cbw);
            }
            ScsiCommand::Inquiry {
                evpd,
                page_code,
                alloc_len,
            } => todo!(),
            ScsiCommand::TestUnitReady => todo!(),
            ScsiCommand::RequestSense { desc, alloc_len } => todo!(),
            ScsiCommand::ModeSense6 {
                dbd,
                page_control,
                page_code,
                subpage_code,
                alloc_len,
            } => todo!(),
            ScsiCommand::ModeSense10 {
                dbd,
                page_control,
                page_code,
                subpage_code,
                alloc_len,
            } => todo!(),
            ScsiCommand::ReadCapacity10 => todo!(),
            ScsiCommand::ReadCapacity16 { alloc_len } => todo!(),
            ScsiCommand::Read { lba, len } => todo!(),
            ScsiCommand::Write { lba, len } => todo!(),
            ScsiCommand::ReadFormatCapacities { alloc_len } => todo!(),
        }
    }
}

#[repr(C, packed)]
struct CommandBlockWrapper {
    dCBWSignature: u32,
    dCBWTag: u32,
    dCBWDataTransferLength: u32,
    bmCBWFlags: u8,
    bCBWLUN: u8,
    bCBWCBLength: u8,
    CBWCB: [u8; 16],
}

impl CommandBlockWrapper {
    fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < 31 {
            return None;
        }
        let dCBWSignature = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        if dCBWSignature != 0x43425355 {
            return None; // invalid signature
        }
        Some(Self {
            dCBWSignature,
            dCBWTag: u32::from_le_bytes(buf[4..8].try_into().unwrap()),
            dCBWDataTransferLength: u32::from_le_bytes(buf[8..12].try_into().unwrap()),
            bmCBWFlags: buf[12],
            bCBWLUN: buf[13],
            bCBWCBLength: buf[14],
            CBWCB: buf[15..31].try_into().unwrap(),
        })
    }
}
