import json
from pathlib import Path
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]
DEPENDENCIES = tomllib.loads((ROOT / "kernel/Cargo.toml").read_text())["dependencies"]
SOURCE = r'''
use std::{cell::{Cell, RefCell}, convert::Infallible, future::Future,
          rc::Rc, task::{Context, Poll, Waker}};
use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use embedded_hal_2::{digital, spi};
use embedded_hal_async::{delay::DelayNs, spi::{Operation, SpiDevice}};
use st7365p_lcd::ST7365P;

mod display {
    pub const SCREEN_WIDTH: usize = 320;
    pub const SCREEN_HEIGHT: usize = 320;
}
#[path = "FRAMEBUFFER_PATH"]
mod framebuffer;

#[derive(Clone, Default)]
struct Pin(Rc<Cell<bool>>);
impl digital::ErrorType for Pin { type Error = Infallible; }
impl digital::OutputPin for Pin {
    fn set_low(&mut self) -> Result<(), Infallible> { self.0.set(false); Ok(()) }
    fn set_high(&mut self) -> Result<(), Infallible> { self.0.set(true); Ok(()) }
}
struct Bus {
    dc: Pin,
    ram_write: bool,
    bytes: Rc<RefCell<Vec<u8>>>,
}
impl spi::ErrorType for Bus { type Error = Infallible; }
impl SpiDevice for Bus {
    async fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Infallible> {
        for operation in operations {
            match operation {
                Operation::Write(bytes) if !self.dc.0.get() => self.ram_write = *bytes == [0x2c],
                Operation::Write(bytes) if self.ram_write => self.bytes.borrow_mut().extend_from_slice(bytes),
                Operation::Write(_) => (),
                Operation::DelayNs(_) => (),
                _ => panic!("unexpected SPI operation"),
            }
        }
        Ok(())
    }
}
struct Delay;
impl DelayNs for Delay { async fn delay_ns(&mut self, _: u32) {} }
fn ready(future: impl Future<Output = Result<(), ()>>) {
    let mut future = std::pin::pin!(future);
    assert_eq!(future.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Ready(Ok(())));
}

#[test]
fn rgb565_reaches_spi_in_big_endian_order() {
    let colors = [Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE, Rgb565::WHITE];
    let expected = [0xf8, 0x00, 0x07, 0xe0, 0x00, 0x1f, 0xff, 0xff];
    let dc = Pin::default();
    let bytes = Rc::new(RefCell::new(Vec::new()));
    let bus = Bus { dc: dc.clone(), ram_write: false, bytes: bytes.clone() };
    let mut display = ST7365P::new(bus, dc, None::<Pin>, true, false, Delay);
    let mut pixels = vec![0; framebuffer::SIZE];
    let mut framebuffer = framebuffer::FrameBuffer::with_storage(&mut pixels[..]);
    framebuffer.clear(Rgb565::BLUE).unwrap();
    ready(framebuffer.draw(&mut display));
    assert_eq!(bytes.borrow().len(), framebuffer::SIZE * 2);
    assert!(bytes.borrow().chunks_exact(2).all(|pixel| pixel == [0, 0x1f]));

    for method in 0..3 {
        bytes.borrow_mut().clear();
        let top = method * 16;
        let area = Rectangle::new(Point::new(0, top), Size::new(4, 1));
        match method {
            0 => framebuffer.fill_contiguous(&area, colors).unwrap(),
            1 => framebuffer.draw_iter(colors.into_iter().enumerate().map(|(index, color)|
                Pixel(Point::new(index as i32, top), color))).unwrap(),
            _ => for (index, color) in colors.into_iter().enumerate() {
                framebuffer.fill_solid(&Rectangle::new(Point::new(index as i32, top),
                    Size::new(1, 1)), color).unwrap();
            },
        }
        ready(framebuffer.partial_draw_batched(&mut display));
        assert_eq!(bytes.borrow().len(), 16 * 16 * 2);
        assert_eq!(&bytes.borrow()[..8], &expected);
    }
    bytes.borrow_mut().clear();
    ready(framebuffer.draw(&mut display));
    assert_eq!(&bytes.borrow()[..8], &expected);
}

#[test]
fn fps_overlay_changes_only_uploaded_snapshot_rows() {
    use framebuffer::fps::{FpsCanvas, FPS_CANVAS_X, FPS_CANVAS_WIDTH, FPS_CANVAS_HEIGHT};
    let mut canvas = FpsCanvas::new();
    canvas.draw_fps(225);
    canvas.draw_iter([Pixel(Point::zero(), Rgb565::RED)]).unwrap();
    let mut snapshot = vec![0x1234; framebuffer::SIZE];
    canvas.composite(&mut snapshot, 0..1);
    assert_eq!(snapshot[FPS_CANVAS_X], 0xf800_u16.swap_bytes());
    assert!(snapshot[..FPS_CANVAS_X].iter().all(|pixel| *pixel == 0x1234));
    assert!(snapshot[FPS_CANVAS_X + FPS_CANVAS_WIDTH..].iter().all(|pixel| *pixel == 0x1234));
    let before = snapshot.clone();
    canvas.composite(&mut snapshot, FPS_CANVAS_HEIGHT..320);
    assert_eq!(snapshot, before);
}

#[test]
fn snapshot_and_overlay_do_not_dirty_live_framebuffer() {
    let mut pixels = vec![0; framebuffer::SIZE];
    let mut framebuffer = framebuffer::FrameBuffer::with_storage(&mut pixels[..]);
    framebuffer.clear(Rgb565::BLUE).unwrap();
    let mut snapshot = vec![0; framebuffer::SIZE];
    let rows = framebuffer.take_dirty_rows(&mut snapshot).unwrap().unwrap();
    framebuffer::fps::FpsCanvas::new().composite(&mut snapshot, rows);
    assert!(framebuffer.dirty_rows().is_none());
    assert!(framebuffer.take_dirty_rows(&mut snapshot).unwrap().is_none());
}
'''


def main():
    manifest = '[package]\nname = "framebuffer-wire-test"\nversion = "0.0.0"\nedition = "2024"\n'
    manifest += '[workspace]\n[features]\nfps = []\npsram = []\n[dependencies]\n'
    for name in ("embedded-graphics", "embedded-hal_2", "embedded-hal-async", "st7365p-lcd", "heapless"):
        dependency = DEPENDENCIES[name]
        if isinstance(dependency, dict) and "path" in dependency:
            dependency = {**dependency, "path": str((ROOT / "kernel" / dependency["path"]).resolve())}
        value = json.dumps(dependency) if isinstance(dependency, str) else "{ " + ", ".join(
            f"{key} = {json.dumps(value)}" for key, value in dependency.items()
        ) + " }"
        manifest += f"{name} = {value}\n"
    manifest += f'userlib_sys = {{ path = {json.dumps(str(ROOT / "userlib_sys"))} }}\n'
    host = next(line.removeprefix("host: ") for line in subprocess.check_output(
        ["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    with tempfile.TemporaryDirectory(prefix="picocalc-framebuffer-") as temporary:
        crate = Path(temporary)
        (crate / "src").mkdir()
        (crate / "Cargo.toml").write_text(manifest)
        (crate / "src/lib.rs").write_text(SOURCE.replace(
            "FRAMEBUFFER_PATH", str(ROOT / "kernel/src/framebuffer.rs")))
        subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(crate / "Cargo.toml"),
                        "--target", host, "--target-dir", str(ROOT / "target/framebuffer-tests"),
                        "--lib", "--features", "fps,psram"], check=True)


if __name__ == "__main__":
    main()
