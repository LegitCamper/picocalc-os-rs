use crate::mtp::MtpClass;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_futures::select::select;
use embassy_rp::{peripherals::USB, usb::Driver};
use embassy_usb::types::StringIndex;
use embassy_usb::{Builder, Config, Handler, UsbDevice};

pub static USB_ACTIVE: AtomicBool = AtomicBool::new(false);

struct MtpStrings(StringIndex);

impl Handler for MtpStrings {
    fn get_string(&mut self, index: StringIndex, _lang_id: u16) -> Option<&str> {
        (index == self.0).then_some("MTP")
    }
}

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
    let mut strings;

    let mut builder = Builder::new(
        driver,
        config,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut [],
        &mut control_buf,
    );

    let interface_name = builder.string();
    strings = MtpStrings(interface_name);
    builder.handler(&mut strings);
    let mut mtp = MtpClass::new(&mut builder, interface_name);
    let usb = builder.build();

    select(run(usb), mtp.run()).await;
}

async fn run<'d>(mut usb: UsbDevice<'d, Driver<'d, USB>>) -> ! {
    loop {
        usb.wait_resume().await;
        USB_ACTIVE.store(true, Ordering::Release);
        usb.run_until_suspend().await;
        USB_ACTIVE.store(false, Ordering::Release);
    }
}
