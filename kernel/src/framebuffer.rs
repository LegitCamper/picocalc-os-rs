use crate::display::{SCREEN_HEIGHT, SCREEN_WIDTH};
use core::sync::atomic::{AtomicBool, Ordering};
use embedded_graphics::{
    draw_target::DrawTarget,
    pixelcolor::{
        Rgb565,
        raw::{RawData, RawU16},
    },
    prelude::*,
    primitives::Rectangle,
};
use embedded_hal_2::digital::OutputPin;
use embedded_hal_async::{delay::DelayNs, spi::SpiDevice};
use st7365p_lcd::ST7365P;

#[cfg(feature = "fps")]
use fps::{FPS_CANVAS, FPS_CANVAS_HEIGHT, FPS_CANVAS_WIDTH, FPS_CANVAS_X, FPS_CANVAS_Y};

const TILE_SIZE: usize = 16; // 16x16 tile
const TILE_COUNT: usize = (SCREEN_WIDTH / TILE_SIZE) * (SCREEN_HEIGHT / TILE_SIZE); // 400 tiles
const NUM_TILE_ROWS: usize = SCREEN_WIDTH / TILE_SIZE;
const NUM_TILE_COLS: usize = SCREEN_WIDTH / TILE_SIZE;

const MAX_BATCH_TILES: usize = (SCREEN_WIDTH / TILE_SIZE) * 2;
type BatchTileBuf = [u16; MAX_BATCH_TILES * TILE_SIZE * TILE_SIZE];

pub const SIZE: usize = SCREEN_HEIGHT * SCREEN_WIDTH;

pub static FB_PAUSED: AtomicBool = AtomicBool::new(false);

#[allow(dead_code)]
pub struct AtomicFrameBuffer<'a> {
    fb: &'a mut [u16],
    dirty_tiles: [AtomicBool; TILE_COUNT],
    batch_tile_buf: BatchTileBuf,
}

impl<'a> AtomicFrameBuffer<'a> {
    pub fn new(buffer: &'a mut [u16]) -> Self {
        assert!(buffer.len() == SIZE);
        Self {
            fb: buffer,
            dirty_tiles: core::array::from_fn(|_| AtomicBool::new(true)),
            batch_tile_buf: [0; MAX_BATCH_TILES * TILE_SIZE * TILE_SIZE],
        }
    }

    fn mark_tiles_dirty(&mut self, rect: Rectangle) {
        let tiles_x = (SCREEN_WIDTH + TILE_SIZE - 1) / TILE_SIZE;
        let start_tx = (rect.top_left.x as usize) / TILE_SIZE;
        let end_tx = ((rect.top_left.x + rect.size.width as i32 - 1) as usize) / TILE_SIZE;
        let start_ty = (rect.top_left.y as usize) / TILE_SIZE;
        let end_ty = ((rect.top_left.y + rect.size.height as i32 - 1) as usize) / TILE_SIZE;

        for ty in start_ty..=end_ty {
            for tx in start_tx..=end_tx {
                let tile_idx = ty * tiles_x + tx;
                self.dirty_tiles[tile_idx].store(true, Ordering::Release);
            }
        }
    }

    fn set_pixels<P: IntoIterator<Item = u16>>(
        &mut self,
        sx: u16,
        sy: u16,
        ex: u16,
        ey: u16,
        colors: P,
    ) -> Result<(), ()> {
        if sx >= self.size().width as u16
            || ex >= self.size().width as u16
            || sy >= self.size().height as u16
            || ey >= self.size().height as u16
        {
            return Err(()); // Bounds check
        }

        let mut color_iter = colors.into_iter();

        for y in sy..=ey {
            for x in sx..=ex {
                if let Some(color) = color_iter.next() {
                    self.fb[(y as usize * SCREEN_WIDTH) + x as usize] = color;
                } else {
                    return Err(()); // Not enough data
                }
            }
        }

        // Optional: check that we consumed *exactly* the right amount
        if color_iter.next().is_some() {
            return Err(()); // Too much data
        }

        Ok(())
    }

    // Checks if a full draw would be faster than individual tile batches
    fn should_full_draw(&self) -> bool {
        let threshold_pixels = SIZE * 80 / 100;
        let mut dirty_pixels = 0;

        self.dirty_tiles.iter().any(|tile| {
            if tile.load(Ordering::Acquire) {
                dirty_pixels += TILE_SIZE * TILE_SIZE;
            }
            dirty_pixels >= threshold_pixels
        })
    }

    /// Sends the entire framebuffer to the display
    pub async fn draw<SPI, DC, RST, DELAY: DelayNs>(
        &mut self,
        display: &mut ST7365P<SPI, DC, RST, DELAY>,
    ) -> Result<(), ()>
    where
        SPI: SpiDevice,
        DC: OutputPin,
        RST: OutputPin,
    {
        display
            .set_pixels_buffered(
                0,
                0,
                self.size().width as u16 - 1,
                self.size().height as u16 - 1,
                &self.fb[..],
            )
            .await?;

        for tile in self.dirty_tiles.iter() {
            tile.store(false, Ordering::Release);
        }

        #[cfg(feature = "fps")]
        unsafe {
            crate::display::FPS_COUNTER.measure()
        }

        Ok(())
    }

    // used when doing a full screen refresh fps must be drawn into fb
    // unfortunately it is not garenteed to not be drawn over before
    // being pushed to the display
    #[cfg(feature = "fps")]
    pub fn draw_fps_into_fb(&mut self) {
        unsafe {
            let canvas = &FPS_CANVAS.canvas;

            for y in 0..FPS_CANVAS_HEIGHT {
                let fb_y = FPS_CANVAS_Y + y;
                let fb_row_start = fb_y * SCREEN_WIDTH + FPS_CANVAS_X;
                let canvas_row_start = y * FPS_CANVAS_WIDTH;

                self.fb[fb_row_start..fb_row_start + FPS_CANVAS_WIDTH].copy_from_slice(
                    &canvas[canvas_row_start..canvas_row_start + FPS_CANVAS_WIDTH],
                );
            }
        }
    }

    // copy N tiles horizontally to the right into batch tile buf
    fn append_tiles_to_batch(
        &mut self,
        tile_x: u16,
        tile_y: u16,
        total_tiles: u16, // number of tiles being written to buf
    ) {
        debug_assert!(total_tiles as usize <= NUM_TILE_COLS);
        for batch_row_num in 0..TILE_SIZE {
            let batch_row_offset = batch_row_num * total_tiles as usize * TILE_SIZE;
            let batch_row = &mut self.batch_tile_buf
                [batch_row_offset..batch_row_offset + (total_tiles as usize * TILE_SIZE)];

            let fb_row_offset = (tile_y as usize * TILE_SIZE + batch_row_num) * SCREEN_WIDTH
                + tile_x as usize * TILE_SIZE;
            let fb_row =
                &self.fb[fb_row_offset..fb_row_offset + (total_tiles as usize * TILE_SIZE)];

            batch_row.copy_from_slice(fb_row);

            // override fps pixel region with fps
            // avoids writing to fps, and having it overridden before draw
            #[cfg(feature = "fps")]
            {
                let global_y = tile_y as usize * TILE_SIZE + batch_row_num;

                if global_y >= FPS_CANVAS_Y && global_y < FPS_CANVAS_Y + FPS_CANVAS_HEIGHT {
                    let start_x = tile_x as usize * TILE_SIZE;
                    let end_x = start_x + (total_tiles as usize * TILE_SIZE);

                    // horizontal overlap check
                    let fps_x0 = FPS_CANVAS_X;
                    let fps_x1 = FPS_CANVAS_X + FPS_CANVAS_WIDTH;

                    let x0 = start_x.max(fps_x0);
                    let x1 = end_x.min(fps_x1);

                    if x1 > x0 {
                        let row_in_fps = global_y - FPS_CANVAS_Y;
                        let fps_off = row_in_fps
                            .checked_mul(FPS_CANVAS_WIDTH)
                            .and_then(|v| v.checked_add(x0 - fps_x0));
                        let batch_off = x0 - start_x;
                        let len = x1 - x0;

                        if let Some(fps_off) = fps_off {
                            let fps_len_ok = fps_off + len <= unsafe { FPS_CANVAS.canvas.len() };
                            let batch_len_ok = batch_off + len <= batch_row.len();

                            if fps_len_ok && batch_len_ok {
                                batch_row[batch_off..batch_off + len].copy_from_slice(unsafe {
                                    &FPS_CANVAS.canvas[fps_off..fps_off + len]
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Pushes tiles to the display in batches to avoid full frame pushes (unless needed)
    pub async fn partial_draw<SPI, DC, RST, DELAY>(
        &mut self,
        display: &mut ST7365P<SPI, DC, RST, DELAY>,
    ) -> Result<(), ()>
    where
        SPI: SpiDevice,
        DC: OutputPin,
        RST: OutputPin,
        DELAY: DelayNs,
    {
        if self.should_full_draw() {
            #[cfg(feature = "fps")]
            self.draw_fps_into_fb();
            return self.draw(display).await;
        }

        #[cfg(feature = "fps")]
        {
            let fps_tile_x = FPS_CANVAS_X / TILE_SIZE;
            let fps_tile_y = FPS_CANVAS_Y / TILE_SIZE;
            let fps_tile_w = (FPS_CANVAS_WIDTH + TILE_SIZE - 1) / TILE_SIZE;
            let fps_tile_h = (FPS_CANVAS_HEIGHT + TILE_SIZE - 1) / TILE_SIZE;

            for ty in fps_tile_y..fps_tile_y + fps_tile_h {
                for tx in fps_tile_x..fps_tile_x + fps_tile_w {
                    self.dirty_tiles[ty * NUM_TILE_COLS + tx].store(true, Ordering::Release);
                }
            }
        }

        for tile_row in 0..NUM_TILE_ROWS {
            let row_start_idx = tile_row * NUM_TILE_COLS;
            let mut col = 0;

            while col < NUM_TILE_COLS {
                // Check for dirty tile
                if self.dirty_tiles[row_start_idx + col].swap(false, Ordering::Acquire) {
                    let run_start = col;
                    let mut run_len = 1;

                    // Extend run while contiguous dirty tiles and within MAX_BATCH_TILES
                    while col + 1 < NUM_TILE_COLS
                        && self.dirty_tiles[row_start_idx + col + 1].load(Ordering::Acquire)
                        && run_len < MAX_BATCH_TILES
                    {
                        col += 1;
                        run_len += 1;
                    }

                    // Copy the whole horizontal run into the batch buffer in one call
                    let tile_x = run_start;
                    let tile_y = tile_row;
                    self.append_tiles_to_batch(tile_x as u16, tile_y as u16, run_len as u16);

                    // Compute coordinates for display write
                    let start_x = tile_x * TILE_SIZE;
                    let end_x = start_x + run_len * TILE_SIZE - 1;
                    let start_y = tile_y * TILE_SIZE;
                    let end_y = start_y + TILE_SIZE - 1;

                    // Send batch to display
                    display
                        .set_pixels_buffered(
                            start_x as u16,
                            start_y as u16,
                            end_x as u16,
                            end_y as u16,
                            &self.batch_tile_buf[..run_len * TILE_SIZE * TILE_SIZE],
                        )
                        .await?;
                }

                col += 1;
            }
        }

        #[cfg(feature = "fps")]
        unsafe {
            crate::display::FPS_COUNTER.measure()
        }

        Ok(())
    }
}

impl<'a> DrawTarget for AtomicFrameBuffer<'a> {
    type Error = ();
    type Color = Rgb565;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        let mut dirty_rect: Option<Rectangle> = None;
        let mut changed = false;

        for Pixel(coord, color) in pixels {
            if coord.x >= 0 && coord.y >= 0 {
                let x = coord.x;
                let y = coord.y;

                if (x as usize) < SCREEN_WIDTH && (y as usize) < SCREEN_HEIGHT {
                    let idx = (y as usize) * SCREEN_WIDTH + (x as usize);
                    let raw_color = RawU16::from(color).into_inner();
                    if self.fb[idx] != raw_color {
                        self.fb[idx] = raw_color;
                        changed = true;
                    }

                    if let Some(ref mut rect) = dirty_rect {
                        rect.top_left.x = rect.top_left.x.min(x);
                        rect.top_left.y = rect.top_left.y.min(y);
                        let max_x = (rect.top_left.x + rect.size.width as i32 - 1).max(x);
                        let max_y = (rect.top_left.y + rect.size.height as i32 - 1).max(y);
                        rect.size.width = (max_x - rect.top_left.x + 1) as u32;
                        rect.size.height = (max_y - rect.top_left.y + 1) as u32;
                    } else {
                        dirty_rect = Some(Rectangle::new(Point::new(x, y), Size::new(1, 1)));
                    }
                }
            }
        }

        if changed {
            if let Some(rect) = dirty_rect {
                self.mark_tiles_dirty(rect);
            }
        }

        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Self::Color>,
    {
        let drawable_area = area.intersection(&Rectangle::new(Point::zero(), self.size()));

        if drawable_area.size != Size::zero() {
            // We assume that `colors` iterator is in row-major order for the original `area`
            // So we must skip rows/pixels that are clipped
            let area_width = area.size.width;
            let area_height = area.size.height;
            let mut colors = colors.into_iter();
            let mut changed = false;

            for y in 0..area_height {
                for x in 0..area_width {
                    let p = area.top_left + Point::new(x as i32, y as i32);

                    if drawable_area.contains(p) {
                        if let Some(color) = colors.next() {
                            let idx = (p.y as usize * SCREEN_WIDTH) + (p.x as usize);
                            let raw_color = RawU16::from(color).into_inner();
                            if self.fb[idx] != raw_color {
                                self.fb[idx] = raw_color;
                                changed = true;
                            }
                        } else {
                            break;
                        }
                    } else {
                        // Still need to consume the color even if not used!
                        let _ = colors.next();
                    }
                }
            }

            if changed {
                self.mark_tiles_dirty(*area);
            }
        }

        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        self.fill_contiguous(
            area,
            core::iter::repeat(color).take((self.size().width * self.size().height) as usize),
        )
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        self.set_pixels(
            0,
            0,
            self.size().width as u16 - 1,
            self.size().height as u16 - 1,
            core::iter::repeat(RawU16::from(color).into_inner())
                .take((self.size().width * self.size().height) as usize),
        )?;

        for tile in self.dirty_tiles.iter() {
            tile.store(true, Ordering::Release);
        }

        Ok(())
    }
}

impl<'a> OriginDimensions for AtomicFrameBuffer<'a> {
    fn size(&self) -> Size {
        Size::new(SCREEN_WIDTH as u32, SCREEN_HEIGHT as u32)
    }
}

#[cfg(feature = "fps")]
pub mod fps {
    use crate::display::SCREEN_WIDTH;
    use core::fmt::Write;
    use embassy_time::{Duration, Instant};
    use embedded_graphics::{
        Drawable, Pixel,
        draw_target::DrawTarget,
        geometry::Point,
        mono_font::{MonoTextStyle, ascii::FONT_8X13},
        pixelcolor::Rgb565,
        prelude::{IntoStorage, OriginDimensions, RgbColor, Size},
        text::{Alignment, Text},
    };

    pub static mut FPS_COUNTER: FpsCounter = FpsCounter::new();
    pub static mut FPS_CANVAS: FpsCanvas = FpsCanvas::new();

    // "FPS: 120" = 8 len
    const FPS_LEN: usize = 8;
    pub const FPS_CANVAS_WIDTH: usize = (FONT_8X13.character_size.width + 4) as usize * FPS_LEN;
    pub const FPS_CANVAS_HEIGHT: usize = FONT_8X13.character_size.height as usize;

    // puts canvas in the top right of the display
    // top left point of canvas
    pub const FPS_CANVAS_X: usize = SCREEN_WIDTH - FPS_CANVAS_WIDTH;
    pub const FPS_CANVAS_Y: usize = 0;

    pub struct FpsCanvas {
        pub canvas: [u16; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
    }

    impl FpsCanvas {
        const fn new() -> Self {
            Self {
                canvas: [0; FPS_CANVAS_HEIGHT * FPS_CANVAS_WIDTH],
            }
        }

        fn clear(&mut self) {
            for p in &mut self.canvas {
                *p = 0;
            }
        }

        pub async fn draw_fps(&mut self) {
            let mut buf: heapless::String<FPS_LEN> = heapless::String::new();
            let fps = unsafe { FPS_COUNTER.smoothed };
            let _ = write!(buf, "FPS: {}", fps as u8);

            self.clear();
            let text_style = MonoTextStyle::new(&FONT_8X13, Rgb565::WHITE);
            Text::with_alignment(
                buf.as_str(),
                Point::new(
                    FPS_CANVAS_WIDTH as i32 / 2,
                    (FPS_CANVAS_HEIGHT as i32 + 8) / 2,
                ),
                text_style,
                Alignment::Center,
            )
            .draw(self)
            .unwrap();
        }
    }

    impl DrawTarget for FpsCanvas {
        type Error = ();
        type Color = Rgb565;

        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
        where
            I: IntoIterator<Item = Pixel<Self::Color>>,
        {
            for Pixel(point, color) in pixels {
                if point.x < 0
                    || point.x >= FPS_CANVAS_WIDTH as i32
                    || point.y < 0
                    || point.y >= FPS_CANVAS_HEIGHT as i32
                {
                    continue;
                }

                let index = (point.y as usize) * FPS_CANVAS_WIDTH + point.x as usize;
                self.canvas[index] = color.into_storage();
            }
            Ok(())
        }
    }

    impl OriginDimensions for FpsCanvas {
        fn size(&self) -> Size {
            Size::new(FPS_CANVAS_WIDTH as u32, FPS_CANVAS_HEIGHT as u32)
        }
    }

    pub struct FpsCounter {
        last_frame: Option<Instant>,
        smoothed: f32,
        last_draw: Option<Instant>,
    }

    impl FpsCounter {
        pub const fn new() -> Self {
            Self {
                last_frame: None,
                smoothed: 0.0,
                last_draw: None,
            }
        }

        // Is called once per frame or partial frame to update FPS
        pub fn measure(&mut self) {
            let now = Instant::now();

            if let Some(last) = self.last_frame {
                let dt_us = (now - last).as_micros() as f32;
                if dt_us > 0.0 {
                    let current = 1_000_000.0 / dt_us;
                    self.smoothed = if self.smoothed == 0.0 {
                        current
                    } else {
                        0.9 * self.smoothed + 0.1 * current
                    };
                }
            }

            self.last_frame = Some(now);
        }

        pub fn should_draw(&mut self) -> bool {
            let now = Instant::now();
            match self.last_draw {
                Some(last) if now - last < Duration::from_millis(200) => false,
                _ => {
                    self.last_draw = Some(now);
                    true
                }
            }
        }
    }
}
