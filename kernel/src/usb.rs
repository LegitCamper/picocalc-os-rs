use crate::scsi::{MassStorageClass, SCSI_BUSY, SCSI_HALT};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_futures::{join::join, select::select};
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::Timer;
use embassy_usb::{Builder, Config};

static START_USB: Channel<CriticalSectionRawMutex, (), 1> = Channel::new();
static STOP_USB: Channel<CriticalSectionRawMutex, (), 1> = Channel::new();

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

    let mut scsi = MassStorageClass::new(&mut builder);
    let mut usb = builder.build();

    loop {
        START_USB.receiver().receive().await;
        USB_ACTIVE.store(true, Ordering::Release);
        SCSI_HALT.store(false, Ordering::Release);
        scsi.take_sdcard().await;
        scsi.pending_eject = false;

        // waits for cancellation signal, and then waits for
        // transfers to stop before dropping usb future
        select(
            async {
                STOP_USB.receiver().receive().await;
                SCSI_HALT.store(true, Ordering::Release);
                while SCSI_BUSY.load(Ordering::Acquire) {
                    Timer::after_millis(100).await;
                }
            },
            async {
                // runs the usb, until cancelled
                join(
                    async {
                        let _ = usb.remote_wakeup().await;
                        usb.run().await
                    },
                    scsi.poll(),
                )
                .await;
            },
        )
        .await;
        usb.disable().await;
        scsi.return_sdcard().await;
        USB_ACTIVE.store(false, Ordering::Release);
    }
}

pub fn start_usb() {
    let _ = STOP_USB.receiver().try_receive();
    let _ = START_USB.sender().try_send(());
}

pub fn stop_usb() {
    let _ = START_USB.receiver().try_receive();
    let _ = STOP_USB.sender().try_send(());
}
