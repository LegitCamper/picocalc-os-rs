use crate::{USB_ENABLED, scsi::MassStorageClass, storage::SdCard};
use embassy_futures::{join::join, select::select3};
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_usb::{Builder, Config};

pub async fn usb_handler(driver: Driver<'static, USB>, sdcard: SdCard) {
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

    loop {
        if USB_ENABLED.wait().await {
            select3(
                async {
                    loop {
                        // stop usb task until usb is enabled again
                        USB_ENABLED.wait().await;
                        return; // breaks out of select
                    }
                },
                usb.run(),
                scsi.poll(),
            )
            .await;
        }
    }
}
