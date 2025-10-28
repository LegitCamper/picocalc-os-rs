use crate::display::{SCREEN_HEIGHT, SCREEN_WIDTH};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::lazy_lock::LazyLock;
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

pub const TILE_SIZE: usize = 16; // 16x16 tile
pub const TILE_COUNT: usize = (SCREEN_WIDTH / TILE_SIZE) * (SCREEN_HEIGHT / TILE_SIZE); // 400 tiles

// Group of tiles for batching
pub const MAX_META_TILES: usize = SCREEN_WIDTH / TILE_SIZE; // max number of meta tiles in buffer
type MetaTileVec = heapless::Vec<Rectangle, { TILE_COUNT / MAX_META_TILES }>;

pub const SIZE: usize = SCREEN_HEIGHT * SCREEN_WIDTH;

static mut DIRTY_TILES: LazyLock<heapless::Vec<AtomicBool, TILE_COUNT>> = LazyLock::new(|| {
    let mut tiles = heapless::Vec::new();
    for _ in 0..TILE_COUNT {
        tiles.push(AtomicBool::new(true)).unwrap();
    }
    tiles
});

#[allow(dead_code)]
pub struct AtomicFrameBuffer<'a>(&'a mut [u16]);

impl<'a> AtomicFrameBuffer<'a> {
    pub fn new(buffer: &'a mut [u16]) -> Self {
        assert!(buffer.len() == SIZE);
        Self(buffer)
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
                unsafe { DIRTY_TILES.get_mut()[tile_idx].store(true, Ordering::Relaxed) };
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
                    self.0[(y as usize * SCREEN_WIDTH) + x as usize] = color;
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

    // walk the dirty tiles and mark groups of tiles(meta-tiles) for batched updates
    fn find_meta_tiles(&mut self, tiles_x: usize, tiles_y: usize) -> MetaTileVec {
        let mut meta_tiles: MetaTileVec = heapless::Vec::new();

        for ty in 0..tiles_y {
            let mut tx = 0;
            while tx < tiles_x {
                let idx = ty * tiles_x + tx;
                if !unsafe { DIRTY_TILES.get()[idx].load(Ordering::Acquire) } {
                    tx += 1;
                    continue;
                }

                // Start meta-tile at this tile
                let mut width_tiles = 1;
                let height_tiles = 1;

                // Grow horizontally, but keep under MAX_TILES_PER_METATILE
                while tx + width_tiles < tiles_x
                    && unsafe {
                        DIRTY_TILES.get()[ty * tiles_x + tx + width_tiles].load(Ordering::Relaxed)
                    }
                    && (width_tiles + height_tiles) <= MAX_META_TILES
                {
                    width_tiles += 1;
                }

                // TODO: for simplicity, skipped vertical growth

                for x_off in 0..width_tiles {
                    unsafe {
                        DIRTY_TILES.get()[ty * tiles_x + tx + x_off]
                            .store(false, Ordering::Release);
                    };
                }

                // new meta-tile pos
                let rect = Rectangle::new(
                    Point::new((tx * TILE_SIZE) as i32, (ty * TILE_SIZE) as i32),
                    Size::new(
                        (width_tiles * TILE_SIZE) as u32,
                        (height_tiles * TILE_SIZE) as u32,
                    ),
                );

                if meta_tiles.push(rect).is_err() {
                    return meta_tiles;
                };

                tx += width_tiles;
            }
        }

        meta_tiles
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
                &self.0[..],
            )
            .await?;

        unsafe {
            for tile in DIRTY_TILES.get_mut().iter() {
                tile.store(false, Ordering::Release);
            }
        };

        Ok(())
    }

    /// Sends only dirty tiles (16x16px) in batches to the display
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
        if unsafe { DIRTY_TILES.get().iter().any(|p| p.load(Ordering::Acquire)) } {
            let tiles_x = (SCREEN_WIDTH + TILE_SIZE - 1) / TILE_SIZE;
            let tiles_y = (SCREEN_HEIGHT + TILE_SIZE - 1) / TILE_SIZE;

            let meta_tiles = self.find_meta_tiles(tiles_x, tiles_y);

            // buffer for copying meta tiles before sending to display
            let mut pixel_buffer: heapless::Vec<u16, { MAX_META_TILES * TILE_SIZE * TILE_SIZE }> =
                heapless::Vec::new();

            for rect in meta_tiles {
                let rect_width = rect.size.width as usize;
                let rect_height = rect.size.height as usize;
                let rect_x = rect.top_left.x as usize;
                let rect_y = rect.top_left.y as usize;

                pixel_buffer.clear();

                for row in 0..rect_height {
                    let y = rect_y + row;
                    let start = y * SCREEN_WIDTH + rect_x;
                    let end = start + rect_width;

                    // Safe: we guarantee buffer will not exceed MAX_META_TILE_PIXELS
                    pixel_buffer.extend_from_slice(&self.0[start..end]).unwrap();
                }

                display
                    .set_pixels_buffered(
                        rect_x as u16,
                        rect_y as u16,
                        (rect_x + rect_width - 1) as u16,
                        (rect_y + rect_height - 1) as u16,
                        &pixel_buffer,
                    )
                    .await?;

                // walk the meta-tile and set as clean
                let start_tx = rect_x / TILE_SIZE;
                let start_ty = rect_y / TILE_SIZE;
                let end_tx = (rect_x + rect_width - 1) / TILE_SIZE;
                let end_ty = (rect_y + rect_height - 1) / TILE_SIZE;

                for ty in start_ty..=end_ty {
                    for tx in start_tx..=end_tx {
                        let tile_idx = ty * tiles_x + tx;
                        unsafe { DIRTY_TILES.get_mut()[tile_idx].store(false, Ordering::Release) };
                    }
                }
            }
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
                let x = coord.x as i32;
                let y = coord.y as i32;

                if (x as usize) < SCREEN_WIDTH && (y as usize) < SCREEN_HEIGHT {
                    let idx = (y as usize) * SCREEN_WIDTH + (x as usize);
                    let raw_color = RawU16::from(color).into_inner();
                    if self.0[idx] != raw_color {
                        self.0[idx] = raw_color;
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
                            if self.0[idx] != raw_color {
                                self.0[idx] = raw_color;
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

        for tile in unsafe { DIRTY_TILES.get_mut() }.iter() {
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
