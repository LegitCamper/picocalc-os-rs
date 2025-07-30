use crate::format;
use embassy_usb::driver::{Driver, EndpointIn, EndpointOut};
use embassy_usb::types::StringIndex;
use embassy_usb::{Builder, Config};
use embedded_sdmmc::{Block, BlockIdx};
use heapless::Vec;

mod scsi_types;
use scsi_types::*;

use crate::storage::SdCard;

const BULK_ENDPOINT_PACKET_SIZE: usize = 64;

pub struct MassStorageClass<'d, D: Driver<'d>> {
    sdcard: SdCard,
    bulk_out: D::EndpointOut,
    bulk_in: D::EndpointIn,
}

impl<'d, D: Driver<'d>> MassStorageClass<'d, D> {
    pub fn new(builder: &mut Builder<'d, D>, sdcard: SdCard) -> Self {
        let mut function = builder.function(0x08, SUBCLASS_SCSI, 0x50); // Mass Storage class
        let mut interface = function.interface();
        let mut alt = interface.alt_setting(0x08, SUBCLASS_SCSI, 0x50, None);

        let bulk_out = alt.endpoint_bulk_out(BULK_ENDPOINT_PACKET_SIZE as u16);
        let bulk_in = alt.endpoint_bulk_in(BULK_ENDPOINT_PACKET_SIZE as u16);

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
                            self.send_csw_fail(cbw.dCBWTag).await
                        }
                    }
                }
            }
        }
    }

    async fn handle_command(&mut self, cbw: &[u8]) -> Result<(), ()> {
        let mut response: Vec<u8, BULK_ENDPOINT_PACKET_SIZE> = Vec::new();
        let mut block = [Block::new(); 1];

        match parse_cb(cbw) {
            ScsiCommand::Unknown => {
                #[cfg(feature = "defmt")]
                defmt::warn!("Got unexpected scsi command: {}", cbw);
                Err(())
            }
            ScsiCommand::Inquiry {
                evpd,
                page_code,
                alloc_len,
            } => {
                if !evpd {
                    response.push(0x00).map_err(|_| ())?; // Direct-access block device
                    response.push(0x80).map_err(|_| ())?; // Removable
                    response.push(0x05).map_err(|_| ())?; // SPC-3 compliance
                    response.push(0x02).map_err(|_| ())?; // Response data format
                    response.push(0x00).map_err(|_| ())?; // Additional length - edited later
                    response.push(0x00).map_err(|_| ())?; // FLAGS
                    response.push(0x00).map_err(|_| ())?; // FLAGS
                    response.push(0).map_err(|_| ())?; // FLAGS
                    assert!(response.len() == 8);

                    let vendor = b"LEGTCMPR";
                    assert!(vendor.len() == 8);
                    response.extend_from_slice(vendor)?;

                    let product = b"Pico Calc Sdcard";
                    assert!(product.len() == 16);
                    response.extend_from_slice(product)?;

                    let version = b"1.00";
                    assert!(version.len() == 4);
                    response.extend_from_slice(version)?; // 4-byte firmware version

                    let addl_len = response.len() - 5;
                    response[4] = addl_len as u8;
                    assert!(response.len() == 36);
                } else {
                    match page_code {
                        0x00 => {
                            response
                                .extend_from_slice(&[
                                    0x00, // Peripheral Qualifier + Peripheral Device Type (0x00 = Direct-access block device)
                                    0x00, // Page Code (same as requested: 0x00)
                                    0x00, 0x03, // Page Length: 3 bytes follow
                                    0x00, // Supported VPD Page: 0x00 (this one — the "Supported VPD Pages" page itself)
                                    0x80, // Supported VPD Page: 0x80 (Unit Serial Number)
                                    0x83, // Supported VPD Page: 0x83 (Device Identification)
                                ])
                                .map_err(|_| ())?
                        }
                        0x80 => {
                            let serial = b"Pico Calc";
                            response.extend_from_slice(&[
                                0x00, // Peripheral Qualifier & Device Type
                                0x80, // Page Code = 0x80 (Unit Serial Number)
                                0x00, // Reserved
                                serial.len() as u8,
                            ])?;
                            response.extend_from_slice(serial)?;
                        }
                        0x83 => {
                            let id = b"SdCard";
                            response.extend_from_slice(&[
                                0x00,
                                0x83, // Page code
                                0x00,
                                (4 + id.len()) as u8, // Length
                                0x02,                 // ASCII identifier
                                0x01,                 // Identifier type
                                0x00,                 // Reserved
                                id.len() as u8,
                            ])?;
                            response.extend_from_slice(id)?;
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
            ScsiCommand::RequestSense { desc, alloc_len } => Ok(()),
            ScsiCommand::ModeSense6 {
                dbd,
                page_control,
                page_code,
                subpage_code,
                alloc_len,
            } => {
                // DBD=0, no block descriptors; total length = 4
                let response = [
                    0x03, // Mode data length (excluding this byte): 3
                    0x00, // Medium type
                    0x00, // Device-specific parameter
                    0x00, // Block descriptor length = 0 (DBD = 1)
                ];

                let len = alloc_len.min(response.len() as u8) as usize;

                self.bulk_in.write(&response[..len]).await.map_err(|_| ())
            }
            ScsiCommand::ModeSense10 {
                dbd,
                page_control,
                page_code,
                subpage_code,
                alloc_len,
            } => {
                let response = [
                    0x00, 0x06, // Mode data length = 6
                    0x00, // Medium type
                    0x00, // Device-specific parameter
                    0x00, 0x00, // Reserved
                    0x00, 0x00, // Block descriptor length = 0
                ];

                let len = alloc_len.min(response.len() as u16) as usize;

                self.bulk_in.write(&response[..len]).await.map_err(|_| ())
            }
            ScsiCommand::ReadCapacity10 => {
                let block_size = SdCard::BLOCK_SIZE as u64;
                let total_blocks = self.sdcard.size() / block_size;

                let last_lba = total_blocks.checked_sub(1).unwrap_or(0);

                response.extend_from_slice(&(last_lba as u32).to_be_bytes())?;
                response.extend_from_slice(&(block_size as u32).to_be_bytes())?;

                self.bulk_in.write(&response).await.map_err(|_| ())
            }
            ScsiCommand::ReadCapacity16 { alloc_len } => {
                let block_size = SdCard::BLOCK_SIZE as u64;
                let total_blocks = self.sdcard.size() / block_size;

                let last_lba = total_blocks.checked_sub(1).unwrap_or(0);

                response.extend_from_slice(&last_lba.to_be_bytes())?; // 8 bytes last LBA
                response.extend_from_slice(&(block_size as u32).to_be_bytes())?; // 4 bytes block length
                response.extend_from_slice(&[0u8; 20])?; // 20 reserved bytes zeroed

                let len = alloc_len.min(response.len() as u32) as usize;
                self.bulk_in.write(&response[..len]).await.map_err(|_| ())
            }
            ScsiCommand::Read { lba, len } => {
                for i in 0..len {
                    let block_idx = BlockIdx(lba as u32 + i as u32);
                    self.sdcard.read_blocks(&mut block, block_idx)?;
                    for chunk in block[0].contents.chunks(BULK_ENDPOINT_PACKET_SIZE.into()) {
                        self.bulk_in.write(chunk).await.map_err(|_| ())?;
                    }
                }
                Ok(())
            }
            ScsiCommand::Write { lba, len } => {
                for i in 0..len {
                    let block_idx = BlockIdx(lba as u32 + i as u32);
                    for chunk in block[0]
                        .contents
                        .chunks_mut(BULK_ENDPOINT_PACKET_SIZE.into())
                    {
                        self.bulk_out.read(chunk).await.map_err(|_| ())?;
                    }
                    self.sdcard.write_blocks(&mut block, block_idx)?;
                }
                Ok(())
            }
            ScsiCommand::ReadFormatCapacities { alloc_len } => {
                let block_size = SdCard::BLOCK_SIZE as u32;
                let num_blocks = (self.sdcard.size() / block_size as u64) as u32;

                let mut response = [0u8; 12];

                // Capacity List Length (8 bytes follows)
                response[3] = 8;

                // Descriptor
                response[4..8].copy_from_slice(&num_blocks.to_be_bytes());
                response[8] = 0x03; // formatted media
                response[9..12].copy_from_slice(&block_size.to_be_bytes()[1..4]); // only 3 bytes

                let response_len = alloc_len.min(response.len() as u16) as usize;
                self.bulk_in
                    .write(&response[..response_len])
                    .await
                    .map_err(|_| ())
            }
            ScsiCommand::PreventAllowMediumRemoval { prevent: _prevent } => Ok(()),
            ScsiCommand::StartStopUnit { start, load_eject } => Ok(()),
        }
    }

    pub async fn send_csw_success(&mut self, tag: u32) {
        self.send_csw(tag, 0x00, 0).await;
    }

    pub async fn send_csw_fail(&mut self, tag: u32) {
        defmt::error!("Command Failed");
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
        let dCBWSignature = u32::from_le_bytes(buf[0..4].try_into().ok()?);
        if dCBWSignature != 0x43425355 {
            return None; // invalid signature
        }
        Some(Self {
            dCBWSignature,
            dCBWTag: u32::from_le_bytes(buf[4..8].try_into().ok()?),
            dCBWDataTransferLength: u32::from_le_bytes(buf[8..12].try_into().ok()?),
            bmCBWFlags: buf[12],
            bCBWLUN: buf[13],
            bCBWCBLength: buf[14],
            CBWCB: buf[15..31].try_into().ok()?,
        })
    }
}
