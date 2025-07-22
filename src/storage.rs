use embassy_rp::gpio::{Input, Output};
use embassy_rp::peripherals::SPI0;
use embassy_rp::spi::{Async, Spi};
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc;
use embedded_sdmmc::asynchronous::{
    Directory, SdCard as SdmmcSdCard, Volume, VolumeIdx, VolumeManager,
};
use embedded_sdmmc::blocking::{TimeSource, Timestamp};

pub const MAX_DIRS: usize = 4;
pub const MAX_FILES: usize = 5;
pub const MAX_VOLUMES: usize = 1;

type Device = ExclusiveDevice<Spi<'static, SPI0, Async>, Output<'static>, embassy_time::Delay>;
type SD = SdmmcSdCard<Device, embassy_time::Delay>;
type VolMgr = VolumeManager<SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;
type Vol<'a> = Volume<'a, SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;
type Dir<'a> = Directory<'a, SD, DummyTimeSource, MAX_DIRS, MAX_FILES, MAX_VOLUMES>;

pub struct DummyTimeSource {}
impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp::from_calendar(2022, 1, 1, 0, 0, 0).unwrap()
    }
}

pub struct SdCard<'a> {
    det: Input<'static>,
    volume_mgr: VolMgr,
    volume: Option<Vol<'a>>,
    root: Option<Dir<'a>>,
}

impl<'a> SdCard<'a> {
    pub fn new(sdcard: SD, det: Input<'static>) -> Self {
        let volume_mgr = VolumeManager::<_, _, MAX_DIRS, MAX_FILES, MAX_VOLUMES>::new_with_limits(
            sdcard,
            DummyTimeSource {},
            5000,
        );
        Self {
            det: det,
            volume_mgr,
            volume: None,
            root: None,
        }
    }

    /// Returns true if an SD card is inserted.
    /// The DET pin is active-low via mechanical switch in the socket.
    fn attached(&self) -> bool {
        self.det.is_low()
    }

    async fn get_root(&'a mut self) {
        let vol = self.volume.as_mut().unwrap();
        let root = vol.open_root_dir().unwrap();
        self.root = Some(root);
    }

    async fn get_volume(&'a mut self) {
        let vol = self.volume_mgr.open_volume(VolumeIdx(0)).await.unwrap();
        self.volume = Some(vol);
    }
}
