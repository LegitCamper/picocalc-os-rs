use crate::{TASK_STATE, TASK_STATE_CHANGED, TaskState, scsi::MassStorageClass};
use embassy_futures::{
    join::join,
    select::{select, select3},
};
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_usb::{Builder, Config};

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
        defmt::info!("in: {}", *TASK_STATE.lock().await as u32);
        if *TASK_STATE.lock().await == TaskState::Ui {
            defmt::info!("running scsi and usb");
            select(join(usb.run(), scsi.poll()), TASK_STATE_CHANGED.wait()).await;
        } else {
            defmt::info!("not in ui state");
            TASK_STATE_CHANGED.wait().await;
        }
    }
}
