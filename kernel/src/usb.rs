use crate::scsi::{MassStorageClass, SCSI_EJECTED};
use core::sync::atomic::Ordering;
use embassy_futures::join::join;
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_usb::{Builder, Config};

#[embassy_executor::task]
pub async fn usb_handler(driver: Driver<'static, USB>) {
    let mut config = Config::new(0xc0de, 0xbabe);
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

    let mut scsi = MassStorageClass::new(&mut builder);
    let mut usb = builder.build();

    join(
        async {
            loop {
                usb.wait_resume().await;
                SCSI_EJECTED.store(false, Ordering::Release);
                usb.run_until_suspend().await;
            }
        },
        scsi.poll(),
    )
    .await;
}
