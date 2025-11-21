use alloc::{string::String, vec::Vec};
use core::str::FromStr;
use embassy_rp::gpio::{Input, Output};
use embassy_rp::peripherals::SPI0;
use embassy_rp::spi::{Blocking, Spi};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::lazy_lock::LazyLock;
use embassy_sync::mutex::Mutex;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{
    Block, BlockDevice, BlockIdx, Directory, SdCard as SdmmcSdCard, TimeSource, Timestamp,
    VolumeIdx, VolumeManager, sdcard::Error,
};
use embedded_sdmmc::{File as SdFile, LfnBuffer, Mode, ShortFileName};

pub const MAX_DIRS: usize = 4;
pub const MAX_FILES: usize = 5;
pub const MAX_VOLUMES: usize = 1;

type Device = ExclusiveDevice<Spi<'static, SPI0, Blocking>, Output<'static>, embassy_time::Delay>;
type SD = SdmmcSdCard<Device, Delay>;
type VolMgr = VolumeManager<SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;
pub type Dir<'a> = Directory<'a, SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;
pub type File<'a> = SdFile<'a, SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;

pub static SDCARD: LazyLock<Mutex<CriticalSectionRawMutex, Option<SdCard>>> =
    LazyLock::new(|| Mutex::new(None));

pub struct DummyTimeSource {}
impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp::from_calendar(2022, 1, 1, 0, 0, 0).unwrap()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct FileName {
    pub long_name: String,
    pub short_name: ShortFileName,
}

impl PartialOrd for FileName {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileName {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.long_name.cmp(&other.long_name)
    }
}

#[derive(Debug)]
pub enum SdCardError {
    Volume0Missing,
    RootDirMissing,
    FileOpenFailed,
}

pub struct SdCard {
    det: Input<'static>,
    volume_mgr: VolMgr,
}

impl SdCard {
    pub const BLOCK_SIZE: u16 = 512;

    pub fn new(sdcard: SD, det: Input<'static>) -> Self {
        let volume_mgr = VolumeManager::<_, _, MAX_DIRS, MAX_FILES, MAX_VOLUMES>::new_with_limits(
            sdcard,
            DummyTimeSource {},
            5000,
        );
        Self { det, volume_mgr }
    }

    /// Returns true if an SD card is inserted.
    /// The DET pin is active-low via mechanical switch in the socket.
    pub fn is_attached(&self) -> bool {
        self.det.is_low()
    }

    pub fn size(&self) -> u64 {
        let mut result = 0;

        self.volume_mgr.device(|sd| {
            result = sd.num_bytes().unwrap_or(0);
            DummyTimeSource {}
        });

        result
    }

    pub fn read_blocks(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), ()> {
        let mut res: Result<(), Error> = Ok(());
        self.volume_mgr.device(|sd| {
            res = sd.read(blocks, start_block_idx);
            DummyTimeSource {}
        });
        res.map_err(|_| ())
    }

    pub fn write_blocks(&self, blocks: &mut [Block], start_block_idx: BlockIdx) -> Result<(), ()> {
        let mut res: Result<(), Error> = Ok(());
        self.volume_mgr.device(|sd| {
            res = sd.write(blocks, start_block_idx);
            DummyTimeSource {}
        });
        res.map_err(|_| ())
    }

    pub fn access_root_dir<R>(
        &mut self,
        mut access: impl FnMut(Dir) -> R,
    ) -> Result<R, SdCardError> {
        let volume0 = self
            .volume_mgr
            .open_volume(VolumeIdx(0))
            .map_err(|_| SdCardError::Volume0Missing)?;
        let root_dir = volume0
            .open_root_dir()
            .map_err(|_| SdCardError::RootDirMissing)?;

        Ok(access(root_dir))
    }

    pub async fn read_file<R>(
        &mut self,
        name: &ShortFileName,
        mut access: impl FnMut(File) -> R,
    ) -> Result<R, SdCardError> {
        self.access_root_dir(|root_dir| {
            let file = root_dir
                .open_file_in_dir(name, Mode::ReadOnly)
                .map_err(|_| SdCardError::FileOpenFailed)?;

            Ok(access(file))
        })?
    }

    /// Returns a Vec of file names (long format) that match the given extension (e.g., "BIN")
    pub fn list_files_by_extension(&mut self, ext: &str) -> Result<Vec<FileName>, SdCardError> {
        let mut result = Vec::new();

        // Only proceed if card is inserted
        if !self.is_attached() {
            return Ok(result);
        }

        let mut lfn_storage = [0; 50];
        let mut lfn_buffer = LfnBuffer::new(&mut lfn_storage);

        self.access_root_dir(|dir| {
            dir.iterate_dir_lfn(&mut lfn_buffer, |entry, name| {
                if let Some(name) = name {
                    let name = String::from_str(name).unwrap();
                    if name.contains(ext) {
                        result.push(FileName {
                            long_name: name,
                            short_name: entry.name.clone(),
                        });
                    }
                }
            })
            .unwrap()
        })?;

        Ok(result)
    }
}
