use crate::{scsi::MassStorageClass, storage::SdCard};
use embassy_futures::join::{self, join};
use embassy_rp::{
    gpio::Output,
    peripherals::{SPI0, USB},
    spi::{Async, Spi},
    usb::Driver,
};
use embassy_time::Delay;
use embassy_usb::{Builder, Config};

pub async fn usb_handler(driver: Driver<'static, USB>, mut sdcard: SdCard<'_>) {
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("LegitCamper");
    config.product = Some("PicoCalc");
    config.serial_number = Some("01001100");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 64];
    let mut control_buf = [0; 64];

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    let mut scsi = MassStorageClass::new(&mut builder, sdcard);
    let mut usb = builder.build();

    join(usb.run(), scsi.poll()).await;
}
