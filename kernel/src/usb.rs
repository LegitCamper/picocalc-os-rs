use core::sync::atomic::{AtomicBool, Ordering};

use crate::{scsi::MassStorageClass, storage::SdCard};
use embassy_futures::{join::join, select::select};
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_usb::{Builder, Config, UsbDevice};

static START_USB: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static STOP_USB: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// for other tasks to query the status of usb (like ui)
// this is read only for ALL other tasks
pub static USB_ACTIVE: AtomicBool = AtomicBool::new(false);

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

    let temp_sd: Option<SdCard> = None;
    let mut scsi = MassStorageClass::new(&mut builder, temp_sd);
    let mut usb = builder.build();

    loop {
        START_USB.wait().await;
        START_USB.reset();
        #[cfg(feature = "defmt")]
        defmt::info!("starting usb");
        select(
            // waits for cancellation signal, and then waits for
            // transfers to stop before dropping usb future
            async {
                STOP_USB.wait().await;
                STOP_USB.reset();
            },
            // runs the usb, until cancelled
            join(usb.run(), scsi.poll()),
        )
        .await;
        #[cfg(feature = "defmt")]
        defmt::info!("disabling usb");
        usb.disable().await;
        USB_ACTIVE.store(false, Ordering::Release);
    }
}

pub fn start_usb() {
    STOP_USB.reset();
    START_USB.signal(());
    USB_ACTIVE.store(true, Ordering::Release);
}

pub fn stop_usb() {
    START_USB.reset();
    STOP_USB.signal(());
    USB_ACTIVE.store(false, Ordering::Release);
}
