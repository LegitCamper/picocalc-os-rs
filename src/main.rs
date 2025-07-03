#![feature(impl_trait_in_assoc_type)]
#![no_std]
#![no_main]

#[cfg(feature = "defmt")]
use defmt::*;
use embedded_graphics::{
    pixelcolor::Rgb565,
    prelude::{Point, RgbColor, Size},
    primitives::{PrimitiveStyle, Rectangle, StyledDrawable},
};
use {defmt_rtt as _, panic_probe as _};

use crate::display::{FRAMEBUFFER, SCREEN_HEIGHT, SCREEN_WIDTH, UI};
use crate::peripherals::{
    conf_peripherals,
    keyboard::{KeyEvent, read_keyboard_fifo},
};
use embassy_executor::Spawner;
use embassy_rp::peripherals::{I2C1, PIN_13, PIN_14, PIN_15, SPI1};
use embassy_rp::spi::Spi;
use embassy_rp::{
    bind_interrupts,
    gpio::{Level, Output},
    i2c,
    i2c::I2c,
    spi,
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Delay, Timer};
use embedded_hal_1::spi::SpiDevice;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_sdmmc::asynchronous::{File, SdCard, ShortFileName, VolumeIdx, VolumeManager};
use st7365p_lcd::{FrameBuffer, ST7365P};
use static_cell::StaticCell;

mod display;
mod peripherals;

embassy_rp::bind_interrupts!(
    struct Irqs {
        I2C1_IRQ => i2c::InterruptHandler<I2C1>;
    }
);

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    let mut i2c1_config = i2c::Config::default();
    i2c1_config.frequency = 100_000;
    let i2c1 = I2c::new_async(p.I2C1, p.PIN_7, p.PIN_6, Irqs, i2c1_config);
    conf_peripherals(i2c1).await;

    let mut spi1_config = spi::Config::default();
    spi1_config.frequency = 16_000_000;
    let spi1 = Spi::new(
        p.SPI1,
        p.PIN_10,
        p.PIN_11,
        p.PIN_12,
        p.DMA_CH0,
        p.DMA_CH1,
        spi1_config,
    );

    spawner
        .spawn(main_task(spi1, p.PIN_13, p.PIN_14, p.PIN_15))
        .unwrap();
}

#[embassy_executor::task]
async fn main_task(
    spi1: Spi<'static, SPI1, spi::Async>,
    spi1_cs: PIN_13,
    spi1_data: PIN_14,
    spi1_reset: PIN_15,
) {
    let spi_device = ExclusiveDevice::new(spi1, Output::new(spi1_cs, Level::Low), Delay).unwrap();
    let display = ST7365P::new(
        spi_device,
        Output::new(spi1_data, Level::Low),
        Some(Output::new(spi1_reset, Level::High)),
        false,
        true,
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    );
    let mut framebuffer: FRAMEBUFFER = FrameBuffer::new(display);
    framebuffer.init(&mut Delay).await.unwrap();
    framebuffer
        .display
        .set_custom_orientation(0x60)
        .await
        .unwrap();

    // let mut ui: UI<50, 25> = UI::new();

    // read_keyboard_fifo().await;
    Rectangle::new(Point::new(0, 0), Size::new(319, 319))
        .draw_styled(&PrimitiveStyle::with_fill(Rgb565::RED), &mut framebuffer)
        .unwrap();
    // ui.draw(&mut framebuffer);
    framebuffer.draw().await.unwrap();

    loop {
        info!("Done");
        Timer::after_millis(500).await;
    }
}
