use embassy_rp::gpio::{Input, Output};
use embassy_rp::peripherals::SPI0;
use embassy_rp::spi::{Blocking, Spi};
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::{
    Block, BlockCount, BlockDevice, BlockIdx, Directory, SdCard as SdmmcSdCard, TimeSource,
    Timestamp, Volume, VolumeIdx, VolumeManager, sdcard::Error,
};

pub const MAX_DIRS: usize = 4;
pub const MAX_FILES: usize = 5;
pub const MAX_VOLUMES: usize = 1;

type Device = ExclusiveDevice<Spi<'static, SPI0, Blocking>, Output<'static>, embassy_time::Delay>;
type SD = SdmmcSdCard<Device, Delay>;
type VolMgr = VolumeManager<SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;
type Vol<'a> = Volume<'a, SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;
type Dir<'a> = Directory<'a, SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;

pub struct DummyTimeSource {}
impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp::from_calendar(2022, 1, 1, 0, 0, 0).unwrap()
    }
}

pub struct SdCard {
    det: Input<'static>,
    volume_mgr: VolMgr,
}

impl SdCard {
    pub const BLOCK_SIZE: u16 = 512;

    pub fn new(sdcard: SD, det: Input<'static>) -> Self {
        sdcard.get_card_type().unwrap();
        defmt::info!("Card size is {} bytes", sdcard.num_bytes().unwrap());
        let volume_mgr = VolumeManager::<_, _, MAX_DIRS, MAX_FILES, MAX_VOLUMES>::new_with_limits(
            sdcard,
            DummyTimeSource {},
            5000,
        );
        Self {
            det: det,
            volume_mgr,
        }
    }

    /// Returns true if an SD card is inserted.
    /// The DET pin is active-low via mechanical switch in the socket.
    pub fn is_attached(&self) -> bool {
        self.det.is_low()
    }

    pub fn open_volume(&mut self) -> Result<Vol<'_>, ()> {
        if self.is_attached() {
            return Ok(self.volume_mgr.open_volume(VolumeIdx(0)).map_err(|_| ())?);
        }
        Err(())
    }

    pub fn size(&self) -> u64 {
        let mut result = 0;

        self.volume_mgr.device(|sd| {
            result = sd.num_bytes().unwrap_or(0);
            DummyTimeSource {}
        });

        result
    }

    pub fn num_blocks(&self) -> u32 {
        let mut result = 0;

        self.volume_mgr.device(|sd| {
            result = sd.num_blocks().unwrap_or(BlockCount(0)).0;
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
            let res = sd.write(blocks, start_block_idx);
            DummyTimeSource {}
        });
        res.map_err(|_| ())
    }
}
