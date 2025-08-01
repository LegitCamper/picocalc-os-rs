use core::sync::atomic::Ordering;

use crate::{scsi::MassStorageClass, storage::SdCard};
use embassy_futures::{
    join::join,
    select::{select, select3},
};
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_sync::{blocking_mutex::raw::ThreadModeRawMutex, signal::Signal};
use embassy_usb::{Builder, Config};
use portable_atomic::AtomicBool;

static RESTART_USB: Signal<ThreadModeRawMutex, ()> = Signal::new();
static ENABLE_SCSI: AtomicBool = AtomicBool::new(false);

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

    if sdcard.is_attached() {
        ENABLE_SCSI.store(true, Ordering::Relaxed);
    }
    let mut scsi = MassStorageClass::new(&mut builder, sdcard);
    let mut usb = builder.build();

    loop {
        select3(
            async {
                loop {
                    RESTART_USB.wait().await;
                    return;
                }
            },
            usb.run(),
            async {
                if ENABLE_SCSI.load(Ordering::Acquire) {
                    scsi.poll().await
                }
            },
        )
        .await;
    }
}
