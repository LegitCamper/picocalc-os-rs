//! USB MTP responder.
//!
//! Shares files over the FAT volume through the kernel's own `SdCard` driver,
//! so the card never has to be handed over to the host as raw blocks and never
//! has to be ejected to be usable locally again.

pub mod codec;

use codec::*;
use core::fmt::Write;
use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, with_timeout};
use embassy_usb::Builder;
use embassy_usb::driver::{Driver, Endpoint, EndpointError, EndpointIn, EndpointOut};
use embassy_usb::types::StringIndex;
use heapless::String;

use crate::storage::SDCARD;

/// Signalled to pause the responder, so a user app has the SD card to itself.
pub static MTP_SHUTDOWN: Signal<CriticalSectionRawMutex, ()> = Signal::new();
/// Signalled to let the responder serve operations again.
pub static MTP_RESUME: Signal<CriticalSectionRawMutex, ()> = Signal::new();

const PACKET_SIZE: u16 = 64;
/// Still-image class, PTP subclass, PTP protocol. MTP rides on this.
const CLASS_IMAGE: u8 = 0x06;
const SUBCLASS_STILL_IMAGE: u8 = 0x01;
const PROTOCOL_PTP: u8 = 0x01;

/// The single storage exposed to the host.
const STORAGE_ID: u32 = 0x0001_0001;
/// Object handles are `FIRST_HANDLE + index of the entry in the root dir`.
const FIRST_HANDLE: u32 = 1;
/// Longest filename accepted or reported.
const MAX_NAME: usize = 64;
/// Scratch buffer for one dataset or one file chunk.
const BUF: usize = 512;
/// Upper bound on root entries addressable as objects.
/// ponytail: root-only, flat listing; add a handle table when subdirs are needed.
const MAX_OBJECTS: u32 = 512;

type Name = String<MAX_NAME>;

/// A pending `SendObjectInfo` -> `SendObject` pair.
struct Pending {
    name: Name,
    size: u32,
}

/// USB MTP responder over a pair of bulk endpoints.
pub struct MtpClass<'d, D: Driver<'d>> {
    bulk_out: D::EndpointOut,
    bulk_in: D::EndpointIn,
    _interrupt_in: D::EndpointIn,
    session_open: bool,
    pending: Option<Pending>,
    buf: [u8; BUF],
}

impl<'d, D: Driver<'d>> MtpClass<'d, D> {
    pub fn new(builder: &mut Builder<'d, D>, interface_name: StringIndex) -> Self {
        let mut function = builder.function(CLASS_IMAGE, SUBCLASS_STILL_IMAGE, PROTOCOL_PTP);
        let mut interface = function.interface();
        let mut alt = interface.alt_setting(
            CLASS_IMAGE,
            SUBCLASS_STILL_IMAGE,
            PROTOCOL_PTP,
            Some(interface_name),
        );

        let bulk_out = alt.endpoint_bulk_out(None, PACKET_SIZE);
        let bulk_in = alt.endpoint_bulk_in(None, PACKET_SIZE);
        let interrupt_in = alt.endpoint_interrupt_in(None, 28, 10);

        Self {
            bulk_out,
            bulk_in,
            _interrupt_in: interrupt_in,
            session_open: false,
            pending: None,
            buf: [0; BUF],
        }
    }

    /// Serve operations forever, pausing between [`MTP_SHUTDOWN`] and
    /// [`MTP_RESUME`] so a running user app owns the SD card alone.
    pub async fn run(&mut self) -> ! {
        loop {
            MTP_SHUTDOWN.reset();
            while let Either::First(()) = select(self.step(), MTP_SHUTDOWN.wait()).await {}

            #[cfg(feature = "defmt")]
            defmt::info!("MTP paused");

            // The host's view of the filesystem is stale after an app runs, so
            // drop the session and make it re-enumerate objects.
            self.session_open = false;
            let _ = self.discard_pending().await;

            MTP_RESUME.reset();
            MTP_RESUME.wait().await;
        }
    }

    /// Read one command container and answer it.
    async fn step(&mut self) {
        self.bulk_out.wait_enabled().await;
        let mut cmd = [0u8; PACKET_SIZE as usize];
        let Ok(n) = self.bulk_out.read(&mut cmd).await else {
            return;
        };
        let Some(hdr) = Header::parse(&cmd[..n]) else {
            return;
        };
        if hdr.kind != CT_COMMAND || hdr.len as usize != n || n > HDR + 4 * 5 || (n - HDR) % 4 != 0
        {
            return;
        }

        let p = params(&cmd[HDR..n]);
        let code = match self.dispatch(hdr, p).await {
            Ok(code) => code,
            Err(code) => {
                #[cfg(feature = "defmt")]
                defmt::warn!("MTP operation failed");
                code
            }
        };
        let _ = self.respond(hdr.tid, code).await;
    }

    /// Run one operation. The `Ok`/`Err` split is only about which response
    /// code to send; both are reported the same way.
    async fn dispatch(&mut self, hdr: Header, p: [u32; 5]) -> Result<u16, u16> {
        // Only session-less operations may run before OpenSession.
        if !self.session_open
            && !matches!(
                hdr.code,
                OP_GET_DEVICE_INFO | OP_OPEN_SESSION | OP_CLOSE_SESSION
            )
        {
            return Err(RC_SESSION_NOT_OPEN);
        }

        match hdr.code {
            OP_GET_DEVICE_INFO => self.send_device_info(hdr.tid).await,
            OP_OPEN_SESSION => {
                if p[0] == 0 {
                    return Err(RC_INVALID_PARAMETER);
                }
                if self.session_open {
                    return Err(RC_SESSION_ALREADY_OPEN);
                }
                self.session_open = true;
                Ok(RC_OK)
            }
            OP_CLOSE_SESSION => {
                self.session_open = false;
                self.discard_pending().await?;
                Ok(RC_OK)
            }
            OP_GET_STORAGE_IDS => {
                self.send_dataset(hdr.tid, OP_GET_STORAGE_IDS, |c| {
                    c.array_u32([STORAGE_ID].into_iter())
                })
                .await
            }
            OP_GET_STORAGE_INFO => {
                if p[0] != STORAGE_ID {
                    return Err(RC_INVALID_STORAGE_ID);
                }
                self.send_storage_info(hdr.tid).await
            }
            OP_GET_NUM_OBJECTS => self.send_num_objects(hdr.tid).await,
            OP_GET_OBJECT_HANDLES => self.send_object_handles(hdr.tid).await,
            OP_GET_OBJECT_INFO => self.send_object_info(hdr.tid, p[0]).await,
            OP_GET_OBJECT => self.send_object(hdr.tid, p[0]).await,
            OP_DELETE_OBJECT => self.delete_object(p[0]).await,
            OP_SEND_OBJECT_INFO => self.recv_object_info(hdr.tid, p[0], p[1]).await,
            OP_SEND_OBJECT => self.recv_object(hdr.tid).await,
            _ => Err(RC_OPERATION_NOT_SUPPORTED),
        }
    }

    /// Write a data container whose payload `fill` produces, then report OK.
    async fn send_dataset(
        &mut self,
        tid: u32,
        operation: u16,
        fill: impl FnOnce(&mut Cursor<'_>) -> Option<()>,
    ) -> Result<u16, u16> {
        let len = {
            let mut cursor = Cursor::new(&mut self.buf[HDR..]);
            fill(&mut cursor).ok_or(RC_GENERAL_ERROR)?;
            cursor.len()
        };

        let total = HDR + len;
        Header {
            len: total as u32,
            kind: CT_DATA,
            code: operation,
            tid,
        }
        .write(&mut self.buf)
        .ok_or(RC_GENERAL_ERROR)?;

        self.write_all(total).await.map_err(|_| RC_GENERAL_ERROR)?;
        Ok(RC_OK)
    }

    async fn send_device_info(&mut self, tid: u32) -> Result<u16, u16> {
        const OPS: [u16; 12] = [
            OP_GET_DEVICE_INFO,
            OP_OPEN_SESSION,
            OP_CLOSE_SESSION,
            OP_GET_STORAGE_IDS,
            OP_GET_STORAGE_INFO,
            OP_GET_NUM_OBJECTS,
            OP_GET_OBJECT_HANDLES,
            OP_GET_OBJECT_INFO,
            OP_GET_OBJECT,
            OP_DELETE_OBJECT,
            OP_SEND_OBJECT_INFO,
            OP_SEND_OBJECT,
        ];

        self.send_dataset(tid, OP_GET_DEVICE_INFO, |c| {
            c.u16(100)?; // standard version 1.00
            c.u32(0x0000_0006)?; // Microsoft MTP vendor extension
            c.u16(100)?; // MTP version 1.00
            c.string("microsoft.com: 1.0;")?;
            c.u16(0)?; // functional mode: standard
            c.array_u16(&OPS)?;
            c.array_u16(&[])?; // events
            c.array_u16(&[])?; // device properties
            c.array_u16(&[])?; // capture formats
            c.array_u16(&[FMT_UNDEFINED, FMT_ASSOCIATION])?;
            c.string("LegitCamper")?;
            c.string("PicoCalc")?;
            c.string("1.0")?;
            c.string("01001100")
        })
        .await
    }

    async fn send_storage_info(&mut self, tid: u32) -> Result<u16, u16> {
        let (capacity, attached) = {
            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
            (sd.size(), sd.is_attached())
        };
        if !attached {
            return Err(RC_STORE_NOT_AVAILABLE);
        }

        self.send_dataset(tid, OP_GET_STORAGE_INFO, |c| {
            c.u16(0x0004)?; // removable RAM
            c.u16(0x0002)?; // generic hierarchical filesystem
            c.u16(0x0000)?; // read-write
            c.u64(capacity)?;
            // ponytail: free space is not tracked by embedded-sdmmc; report
            // capacity so hosts allow writes. Compute from the FAT when it matters.
            c.u64(capacity)?;
            c.u32(u32::MAX)?; // free objects: unknown
            c.string("SD Card")?;
            c.string("")
        })
        .await
    }

    /// Index-to-name mapping for the root directory. Handles are positional,
    /// so a listing must happen before any handle is used.
    async fn name_of(&self, handle: u32) -> Result<(Name, u32, bool), u16> {
        let index = handle
            .checked_sub(FIRST_HANDLE)
            .ok_or(RC_INVALID_OBJECT_HANDLE)?;
        let mut found = None;
        let mut seen = 0u32;

        let mut guard = SDCARD.get().lock().await;
        let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
        sd.list_root(|name, size, is_dir| {
            if seen == index {
                let mut out = Name::new();
                if out.push_str(name).is_ok() {
                    found = Some((out, size, is_dir));
                }
            }
            seen += 1;
        })
        .map_err(|_| RC_STORE_NOT_AVAILABLE)?;

        found.ok_or(RC_INVALID_OBJECT_HANDLE)
    }

    async fn send_num_objects(&mut self, tid: u32) -> Result<u16, u16> {
        let mut count = 0u32;
        {
            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
            sd.list_root(|_, _, _| count += 1)
                .map_err(|_| RC_STORE_NOT_AVAILABLE)?;
        }
        self.respond_with(tid, RC_OK, &[count])
            .await
            .map_err(|_| RC_GENERAL_ERROR)?;
        // Already answered with parameters; suppress the plain response.
        Err(RC_OK_ALREADY_SENT)
    }

    async fn send_object_handles(&mut self, tid: u32) -> Result<u16, u16> {
        let mut count = 0u32;
        {
            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
            sd.list_root(|_, _, _| count += 1)
                .map_err(|_| RC_STORE_NOT_AVAILABLE)?;
        }
        let count = count.min(MAX_OBJECTS);

        self.send_dataset(tid, OP_GET_OBJECT_HANDLES, |c| {
            c.array_u32(FIRST_HANDLE..FIRST_HANDLE + count)
        })
        .await
    }

    async fn send_object_info(&mut self, tid: u32, handle: u32) -> Result<u16, u16> {
        let (name, size, is_dir) = self.name_of(handle).await?;

        self.send_dataset(tid, OP_GET_OBJECT_INFO, |c| {
            c.u32(STORAGE_ID)?;
            c.u16(if is_dir {
                FMT_ASSOCIATION
            } else {
                FMT_UNDEFINED
            })?;
            c.u16(0)?; // no protection
            c.u32(size)?;
            c.u16(0)?; // thumb format
            c.u32(0)?; // thumb compressed size
            c.u32(0)?; // thumb width
            c.u32(0)?; // thumb height
            c.u32(0)?; // image width
            c.u32(0)?; // image height
            c.u32(0)?; // image bit depth
            c.u32(0)?; // parent: root
            c.u16(if is_dir { 1 } else { 0 })?; // association type
            c.u32(0)?; // association description
            c.u32(0)?; // sequence number
            c.string(&name)?;
            c.string("")?; // date created
            c.string("")?; // date modified
            c.string("") // keywords
        })
        .await
    }

    /// Stream a file straight from the card into the bulk endpoint.
    async fn send_object(&mut self, tid: u32, handle: u32) -> Result<u16, u16> {
        let (name, size, is_dir) = self.name_of(handle).await?;
        if is_dir {
            return Err(RC_INVALID_OBJECT_HANDLE);
        }

        let total = size.checked_add(HDR as u32).ok_or(RC_GENERAL_ERROR)?;
        Header {
            len: total,
            kind: CT_DATA,
            code: OP_GET_OBJECT,
            tid,
        }
        .write(&mut self.buf)
        .ok_or(RC_GENERAL_ERROR)?;

        let mut sent = 0u32;
        let mut prefix = HDR;
        loop {
            let want = (size - sent).min((BUF - prefix) as u32) as usize;
            let read = if want == 0 {
                0
            } else {
                let mut guard = SDCARD.get().lock().await;
                let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
                sd.read_root_file(&name, sent, &mut self.buf[prefix..prefix + want])
                    .map_err(|_| RC_GENERAL_ERROR)?
            };
            if read != want {
                return Err(RC_GENERAL_ERROR);
            }
            for chunk in self.buf[..prefix + read].chunks(PACKET_SIZE as usize) {
                self.bulk_in
                    .write(chunk)
                    .await
                    .map_err(|_| RC_GENERAL_ERROR)?;
            }
            sent += read as u32;
            prefix = 0;
            if sent == size {
                break;
            }
        }

        // A transfer that is a whole multiple of the packet size needs a ZLP so
        // the host knows it ended.
        if total % PACKET_SIZE as u32 == 0 {
            self.bulk_in
                .write(&[])
                .await
                .map_err(|_| RC_GENERAL_ERROR)?;
        }
        Ok(RC_OK)
    }

    async fn delete_object(&mut self, handle: u32) -> Result<u16, u16> {
        let (name, _, is_dir) = self.name_of(handle).await?;
        if is_dir {
            // ponytail: directory removal is unsupported by embedded-sdmmc 0.9.
            return Err(RC_OPERATION_NOT_SUPPORTED);
        }

        let mut guard = SDCARD.get().lock().await;
        let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
        sd.delete_root_file(&name).map_err(|_| RC_GENERAL_ERROR)?;
        Ok(RC_OK)
    }

    /// Accept the ObjectInfo dataset that precedes `SendObject`.
    async fn recv_object_info(&mut self, tid: u32, storage: u32, parent: u32) -> Result<u16, u16> {
        let len = self.read_data_container(OP_SEND_OBJECT_INFO, tid).await?;
        self.discard_pending().await?;
        if storage != 0 && storage != STORAGE_ID {
            return Err(RC_INVALID_STORAGE_ID);
        }
        if parent != 0 && parent != u32::MAX {
            return Err(RC_INVALID_PARENT_OBJECT);
        }

        let mut raw = [0u8; MAX_NAME];
        let info =
            parse_object_info(&self.buf[HDR..len], &mut raw).ok_or(RC_NO_VALID_OBJECT_INFO)?;
        let name =
            core::str::from_utf8(&raw[..info.name_len]).map_err(|_| RC_NO_VALID_OBJECT_INFO)?;
        let short = embedded_sdmmc::ShortFileName::create_from_str(name)
            .map_err(|_| RC_NO_VALID_OBJECT_INFO)?;
        info.size
            .checked_add(HDR as u32)
            .ok_or(RC_NO_VALID_OBJECT_INFO)?;
        let mut stored = Name::new();
        write!(&mut stored, "{short}").map_err(|_| RC_NO_VALID_OBJECT_INFO)?;
        if !stored.eq_ignore_ascii_case(name) {
            return Err(RC_NO_VALID_OBJECT_INFO);
        }

        let handle = {
            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
            let mut exists = false;
            sd.list_root(|name, _, _| exists |= name.eq_ignore_ascii_case(&stored))
                .map_err(|_| RC_STORE_NOT_AVAILABLE)?;
            if exists {
                return Err(RC_INVALID_PARAMETER);
            }
            if info.is_dir {
                sd.make_root_dir(&stored).map_err(|_| RC_GENERAL_ERROR)?;
            } else {
                sd.create_root_file(&stored).map_err(|_| RC_GENERAL_ERROR)?;
                self.pending = Some(Pending {
                    name: stored.clone(),
                    size: info.size,
                });
            }
            let mut handle = None;
            let mut index = FIRST_HANDLE;
            sd.list_root(|name, _, _| {
                if name.eq_ignore_ascii_case(&stored) {
                    handle = Some(index);
                }
                index += 1;
            })
            .map_err(|_| RC_STORE_NOT_AVAILABLE)?;
            handle.ok_or(RC_GENERAL_ERROR)?
        };

        self.respond_with(tid, RC_OK, &[STORAGE_ID, 0, handle])
            .await
            .map_err(|_| RC_GENERAL_ERROR)?;
        Ok(RC_OK_ALREADY_SENT)
    }

    /// Receive file data and append it to the pending object.
    async fn recv_object(&mut self, tid: u32) -> Result<u16, u16> {
        let pending = self.pending.as_ref().ok_or(RC_NO_VALID_OBJECT_INFO)?;
        let name = pending.name.clone();
        let size = pending.size;
        let result = self.recv_file_data(tid, &name, size).await;
        if result.is_ok() {
            self.pending = None;
        } else {
            self.discard_pending().await?;
        }
        result
    }

    async fn recv_file_data(&mut self, tid: u32, name: &str, size: u32) -> Result<u16, u16> {
        let mut packet = [0u8; PACKET_SIZE as usize];
        let received = self.read_packet(&mut packet).await?;
        let (mut phase, header) = DataPhase::start(&packet[..received], OP_SEND_OBJECT, tid)
            .ok_or(RC_INCOMPLETE_TRANSFER)?;
        let mut failure = (header.len - HDR as u32 != size).then_some(RC_INCOMPLETE_TRANSFER);
        let mut filled = received - HDR;
        self.buf[..filled].copy_from_slice(&packet[HDR..received]);

        loop {
            if filled > BUF - PACKET_SIZE as usize || phase.remaining == 0 {
                if failure.is_none() && filled != 0 {
                    let mut guard = SDCARD.get().lock().await;
                    let written = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE).and_then(|sd| {
                        sd.append_root_file(name, &self.buf[..filled])
                            .map_err(|_| RC_GENERAL_ERROR)
                    });
                    failure = written.err();
                }
                filled = 0;
            }
            if phase.remaining == 0 {
                break;
            }
            let received = self.read_packet(&mut packet).await?;
            phase.accept(received).ok_or(RC_INCOMPLETE_TRANSFER)?;
            self.buf[filled..filled + received].copy_from_slice(&packet[..received]);
            filled += received;
        }

        failure.map_or(Ok(RC_OK), Err)
    }

    /// Read a whole data container into `self.buf`, returning its used length.
    async fn read_data_container(&mut self, operation: u16, tid: u32) -> Result<usize, u16> {
        let mut packet = [0u8; PACKET_SIZE as usize];
        let mut filled = self.read_packet(&mut packet).await?;
        let (mut phase, header) =
            DataPhase::start(&packet[..filled], operation, tid).ok_or(RC_INCOMPLETE_TRANSFER)?;
        let fits = header.len as usize <= BUF;
        self.buf[..filled].copy_from_slice(&packet[..filled]);
        while phase.remaining > 0 {
            let received = self.read_packet(&mut packet).await?;
            phase.accept(received).ok_or(RC_INCOMPLETE_TRANSFER)?;
            if fits {
                self.buf[filled..filled + received].copy_from_slice(&packet[..received]);
                filled += received;
            }
        }
        if fits {
            Ok(filled)
        } else {
            Err(RC_NO_VALID_OBJECT_INFO)
        }
    }

    async fn read_packet(&mut self, packet: &mut [u8; PACKET_SIZE as usize]) -> Result<usize, u16> {
        with_timeout(Duration::from_secs(5), self.bulk_out.read(packet))
            .await
            .map_err(|_| RC_INCOMPLETE_TRANSFER)?
            .map_err(|_| RC_INCOMPLETE_TRANSFER)
    }

    async fn discard_pending(&mut self) -> Result<(), u16> {
        if let Some(pending) = self.pending.as_ref() {
            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
            sd.delete_root_file(&pending.name)
                .map_err(|_| RC_GENERAL_ERROR)?;
            self.pending = None;
        }
        Ok(())
    }

    async fn respond(&mut self, tid: u32, code: u16) -> Result<(), EndpointError> {
        if code == RC_OK_ALREADY_SENT {
            return Ok(());
        }
        self.respond_with(tid, code, &[]).await
    }

    async fn respond_with(
        &mut self,
        tid: u32,
        code: u16,
        extra: &[u32],
    ) -> Result<(), EndpointError> {
        let mut out = [0u8; HDR + 4 * 5];
        let len = HDR + extra.len() * 4;
        Header {
            len: len as u32,
            kind: CT_RESPONSE,
            code,
            tid,
        }
        .write(&mut out)
        .expect("response header fits a fixed buffer");
        for (i, v) in extra.iter().enumerate() {
            out[HDR + i * 4..HDR + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        self.bulk_in.write(&out[..len]).await
    }

    async fn write_all(&mut self, len: usize) -> Result<(), EndpointError> {
        for chunk in self.buf[..len].chunks(PACKET_SIZE as usize) {
            self.bulk_in.write(chunk).await?;
        }
        if len % PACKET_SIZE as usize == 0 {
            self.bulk_in.write(&[]).await?;
        }
        Ok(())
    }
}

/// Internal marker: the operation already wrote its own response container.
const RC_OK_ALREADY_SENT: u16 = 0xFFFF;
