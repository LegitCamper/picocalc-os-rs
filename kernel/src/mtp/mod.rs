//! USB MTP responder.
//!
//! Shares files over the FAT volume through the kernel's own `SdCard` driver,
//! so the card never has to be handed over to the host as raw blocks and never
//! has to be ejected to be usable locally again.

pub mod codec;

use codec::*;
use embassy_futures::select::{Either, select};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_usb::Builder;
use embassy_usb::driver::{Driver, EndpointError, EndpointIn, EndpointOut};
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
    is_dir: bool,
}

/// USB MTP responder over a pair of bulk endpoints.
pub struct MtpClass<'d, D: Driver<'d>> {
    bulk_out: D::EndpointOut,
    bulk_in: D::EndpointIn,
    session_open: bool,
    pending: Option<Pending>,
    buf: [u8; BUF],
}

impl<'d, D: Driver<'d>> MtpClass<'d, D> {
    pub fn new(builder: &mut Builder<'d, D>) -> Self {
        let mut function = builder.function(CLASS_IMAGE, SUBCLASS_STILL_IMAGE, PROTOCOL_PTP);
        let mut interface = function.interface();
        let mut alt =
            interface.alt_setting(CLASS_IMAGE, SUBCLASS_STILL_IMAGE, PROTOCOL_PTP, None);

        let bulk_out = alt.endpoint_bulk_out(None, PACKET_SIZE);
        let bulk_in = alt.endpoint_bulk_in(None, PACKET_SIZE);

        Self {
            bulk_out,
            bulk_in,
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
            self.pending = None;

            MTP_RESUME.reset();
            MTP_RESUME.wait().await;
        }
    }

    /// Read one command container and answer it.
    async fn step(&mut self) {
        let mut cmd = [0u8; HDR + 4 * 5];
        let Ok(n) = self.bulk_out.read(&mut cmd).await else {
            return;
        };
        let Some(hdr) = Header::parse(&cmd[..n]) else {
            return;
        };
        if hdr.kind != CT_COMMAND {
            return;
        }

        let p = params(&cmd[HDR..n]);
        let code = match self.dispatch(hdr, p).await {
            Ok(code) => code,
            Err(code) => code,
        };
        let _ = self.respond(hdr.tid, code).await;
    }

    /// Run one operation. The `Ok`/`Err` split is only about which response
    /// code to send; both are reported the same way.
    async fn dispatch(&mut self, hdr: Header, p: [u32; 5]) -> Result<u16, u16> {
        // Only session-less operations may run before OpenSession.
        if !self.session_open
            && !matches!(hdr.code, OP_GET_DEVICE_INFO | OP_OPEN_SESSION | OP_CLOSE_SESSION)
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
                self.pending = None;
                Ok(RC_OK)
            }
            OP_GET_STORAGE_IDS => self.send_dataset(hdr.tid, |c| c.array_u32([STORAGE_ID].into_iter())).await,
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
            OP_SEND_OBJECT_INFO => self.recv_object_info(p[0]).await,
            OP_SEND_OBJECT => self.recv_object().await,
            _ => Err(RC_OPERATION_NOT_SUPPORTED),
        }
    }

    /// Write a data container whose payload `fill` produces, then report OK.
    async fn send_dataset(
        &mut self,
        tid: u32,
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
            code: 0,
            tid,
        }
        .write(&mut self.buf)
        .ok_or(RC_GENERAL_ERROR)?;

        // `code` in a data container echoes the operation; hosts ignore it, but
        // keep it zero-free by reusing the response slot below instead.
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

        self.send_dataset(tid, |c| {
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

        self.send_dataset(tid, |c| {
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
        let index = handle.checked_sub(FIRST_HANDLE).ok_or(RC_INVALID_OBJECT_HANDLE)?;
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
        self.respond_with(tid, RC_OK, &[count]).await.map_err(|_| RC_GENERAL_ERROR)?;
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

        self.send_dataset(tid, |c| {
            c.array_u32(FIRST_HANDLE..FIRST_HANDLE + count)
        })
        .await
    }

    async fn send_object_info(&mut self, tid: u32, handle: u32) -> Result<u16, u16> {
        let (name, size, is_dir) = self.name_of(handle).await?;

        self.send_dataset(tid, |c| {
            c.u32(STORAGE_ID)?;
            c.u16(if is_dir { FMT_ASSOCIATION } else { FMT_UNDEFINED })?;
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

        let total = HDR as u32 + size;
        let mut head = [0u8; HDR];
        Header {
            len: total,
            kind: CT_DATA,
            code: OP_GET_OBJECT,
            tid,
        }
        .write(&mut head)
        .ok_or(RC_GENERAL_ERROR)?;
        self.bulk_in.write(&head).await.map_err(|_| RC_GENERAL_ERROR)?;

        let mut sent = 0u32;
        while sent < size {
            let want = (size - sent).min(BUF as u32) as usize;
            let read = {
                let mut guard = SDCARD.get().lock().await;
                let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
                sd.read_root_file(&name, sent, &mut self.buf[..want])
                    .map_err(|_| RC_GENERAL_ERROR)?
            };
            if read == 0 {
                return Err(RC_GENERAL_ERROR);
            }
            for chunk in self.buf[..read].chunks(PACKET_SIZE as usize) {
                self.bulk_in.write(chunk).await.map_err(|_| RC_GENERAL_ERROR)?;
            }
            sent += read as u32;
        }

        // A transfer that is a whole multiple of the packet size needs a ZLP so
        // the host knows it ended.
        if total % PACKET_SIZE as u32 == 0 {
            self.bulk_in.write(&[]).await.map_err(|_| RC_GENERAL_ERROR)?;
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
    async fn recv_object_info(&mut self, storage: u32) -> Result<u16, u16> {
        if storage != 0 && storage != STORAGE_ID {
            return Err(RC_INVALID_STORAGE_ID);
        }

        let len = self.read_data_container().await.map_err(|_| RC_GENERAL_ERROR)?;
        let mut raw = [0u8; MAX_NAME];
        let info = parse_object_info(&self.buf[HDR..len], &mut raw).ok_or(RC_NO_VALID_OBJECT_INFO)?;
        let name = core::str::from_utf8(&raw[..info.name_len]).map_err(|_| RC_NO_VALID_OBJECT_INFO)?;
        let mut stored = Name::new();
        stored.push_str(name).map_err(|_| RC_NO_VALID_OBJECT_INFO)?;

        if info.is_dir {
            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
            sd.make_root_dir(&stored).map_err(|_| RC_STORE_FULL)?;
        }

        self.pending = Some(Pending {
            name: stored,
            is_dir: info.is_dir,
        });
        Ok(RC_OK)
    }

    /// Receive file data and append it to the pending object.
    async fn recv_object(&mut self) -> Result<u16, u16> {
        let pending = self.pending.take().ok_or(RC_NO_VALID_OBJECT_INFO)?;
        if pending.is_dir {
            return Ok(RC_OK);
        }

        let mut head = [0u8; HDR];
        let n = self.bulk_out.read(&mut head).await.map_err(|_| RC_GENERAL_ERROR)?;
        let hdr = Header::parse(&head[..n]).ok_or(RC_GENERAL_ERROR)?;
        let mut left = (hdr.len as usize).saturating_sub(HDR);
        let mut first = true;

        while left > 0 {
            let mut filled = 0;
            while filled < BUF && left > 0 {
                let take = (BUF - filled).min(PACKET_SIZE as usize);
                let got = self
                    .bulk_out
                    .read(&mut self.buf[filled..filled + take])
                    .await
                    .map_err(|_| RC_GENERAL_ERROR)?;
                if got == 0 {
                    break;
                }
                filled += got;
                left = left.saturating_sub(got);
            }

            let mut guard = SDCARD.get().lock().await;
            let sd = guard.as_mut().ok_or(RC_STORE_NOT_AVAILABLE)?;
            sd.write_root_file(&pending.name, first, &self.buf[..filled])
                .map_err(|_| RC_STORE_FULL)?;
            first = false;
        }

        Ok(RC_OK)
    }

    /// Read a whole data container into `self.buf`, returning its used length.
    async fn read_data_container(&mut self) -> Result<usize, EndpointError> {
        let mut filled = 0;
        loop {
            let take = (BUF - filled).min(PACKET_SIZE as usize);
            if take == 0 {
                return Ok(filled);
            }
            let got = self.bulk_out.read(&mut self.buf[filled..filled + take]).await?;
            filled += got;

            if let Some(hdr) = Header::parse(&self.buf[..filled]) {
                if filled >= hdr.len as usize || got < PACKET_SIZE as usize {
                    return Ok(filled.min(hdr.len as usize));
                }
            } else if got < PACKET_SIZE as usize {
                return Ok(filled);
            }
        }
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
