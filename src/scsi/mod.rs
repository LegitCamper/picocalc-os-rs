use crate::format;
use embassy_usb::driver::{Driver, EndpointIn, EndpointOut};
use embassy_usb::types::StringIndex;
use embassy_usb::{Builder, Config};
use heapless::Vec;

mod scsi_types;
use scsi_types::*;

use crate::storage::SdCard;

pub struct MassStorageClass<'d, D: Driver<'d>> {
    sdcard: SdCard,
    bulk_out: D::EndpointOut,
    bulk_in: D::EndpointIn,
}

impl<'d, D: Driver<'d>> MassStorageClass<'d, D> {
    pub fn new(builder: &mut Builder<'d, D>, sdcard: SdCard) -> Self {
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
        loop {
            let mut cbw_buf = [0u8; 31];
            if let Ok(n) = self.bulk_out.read(&mut cbw_buf).await {
                if n == 31 {
                    if let Some(cbw) = CommandBlockWrapper::parse(&cbw_buf[..n]) {
                        // TODO: validate cbw
                        if self.handle_command(&cbw.CBWCB).await.is_ok() {
                            self.send_csw_success(cbw.dCBWTag).await
                        } else {
                            self.send_csw_success(cbw.dCBWTag).await
                        }
                    }
                }
            }
        }
    }

    async fn handle_command(&mut self, cbw: &[u8]) -> Result<(), ()> {
        match parse_cb(cbw) {
            ScsiCommand::Unknown => {
                #[cfg(feature = "defmt")]
                defmt::info!("Got unexpected scsi command: {}", cbw);
                Err(())
            }
            ScsiCommand::Inquiry {
                evpd,
                page_code,
                alloc_len,
            } => {
                #[cfg(feature = "defmt")]
                defmt::info!(
                    "SCSI INQUIRY: evpd={}, page_code=0x{:02x}, alloc_len={}",
                    evpd,
                    page_code,
                    alloc_len
                );

                let mut response: Vec<u8, 64> = Vec::new();
                if !evpd {
                    response.push(0x00).unwrap(); // Direct-access block device
                    response.push(0x80).unwrap(); // Removable
                    response.push(0x05).unwrap(); // SPC-3 compliance
                    response.push(0x02).unwrap(); // Response data format
                    response.push(0).unwrap(); // Additional length

                    // Vendor ID (8 bytes)
                    response.extend_from_slice(b"RUSTUSB ").unwrap();
                    // Product ID (16 bytes)
                    response.extend_from_slice(b"Mass Storage    ").unwrap();

                    // Product Revision (4 bytes): encode volume size in GB
                    let size_bytes = self.sdcard.size();
                    let size_gb = ((size_bytes + 500_000_000) / 1_000_000_000) as u32;
                    let rev_str = format!(4, "{}", size_gb);

                    let rev_bytes = rev_str.as_bytes();
                    response
                        .extend_from_slice(&[
                            *rev_bytes.get(0).unwrap_or(&b'0'),
                            *rev_bytes.get(1).unwrap_or(&b'0'),
                            *rev_bytes.get(2).unwrap_or(&b'0'),
                            *rev_bytes.get(3).unwrap_or(&b'0'),
                        ])
                        .unwrap();

                    // Now fix up the Additional Length
                    let addl_len = response.len() - 5;
                    response[4] = addl_len as u8;
                } else {
                    match page_code {
                        0x00 => {
                            response
                                .extend_from_slice(&[0x00, 0x00, 0x00, 0x03, 0x00, 0x80, 0x83])
                                .unwrap();
                        }
                        0x80 => {
                            let serial = b"RUST1234";
                            let mut data: Vec<u8, 64> = Vec::new();
                            data.extend_from_slice(&[0x00, 0x80, 0x00, serial.len() as u8])
                                .unwrap();
                            data.extend_from_slice(serial).unwrap();
                        }
                        0x83 => {
                            let id = b"RUSTVOL1";
                            let mut data: Vec<u8, 64> = Vec::new();
                            data.extend_from_slice(&[
                                0x00,
                                0x83, // Page code
                                0x00,
                                (4 + id.len()) as u8, // Length
                                0x02,                 // ASCII identifier
                                0x01,                 // Identifier type
                                0x00,                 // Reserved
                                id.len() as u8,
                            ])
                            .unwrap();
                            data.extend_from_slice(id).unwrap();
                        }
                        _ => (),
                    }
                };

                let len = core::cmp::min(alloc_len as usize, response.len());
                self.bulk_in.write(&response[..len]).await.map_err(|_| ())
            }
            ScsiCommand::TestUnitReady => {
                if self.sdcard.is_attached() {
                    Ok(())
                } else {
                    Err(())
                }
            }
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
            ScsiCommand::ReadCapacity10 => {
                const block_size: u64 = 512;
                let total_blocks = self.sdcard.size() / block_size;
                defmt::info!("total size: {}", self.sdcard.size());

                let last_lba = total_blocks - 1;

                let mut resp = [0u8; 8];
                resp[0..4].copy_from_slice(&(last_lba as u32).to_be_bytes());
                resp[4..8].copy_from_slice(&(block_size as u32).to_be_bytes());

                self.bulk_in.write(&resp).await.map_err(|_| ())
            }
            ScsiCommand::ReadCapacity16 { alloc_len } => todo!(),
            ScsiCommand::Read { lba, len } => todo!(),
            ScsiCommand::Write { lba, len } => todo!(),
            ScsiCommand::ReadFormatCapacities { alloc_len } => todo!(),
        }
    }

    pub async fn send_csw_success(&mut self, tag: u32) {
        self.send_csw(tag, 0x00, 0).await;
    }

    pub async fn send_csw_fail(&mut self, tag: u32) {
        self.send_csw(tag, 0x01, 0).await; // 0x01 = Command Failed
    }

    pub async fn send_csw(&mut self, tag: u32, status: u8, residue: u32) {
        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&0x53425355u32.to_le_bytes()); // Signature "USBS"
        csw[4..8].copy_from_slice(&tag.to_le_bytes());
        csw[8..12].copy_from_slice(&residue.to_le_bytes());
        csw[12] = status;
        let _ = self.bulk_in.write(&csw).await;
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
