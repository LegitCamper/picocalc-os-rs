use alloc::vec::Vec;
use core::ops::Range;
use embedded_graphics::{
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::*,
    primitives::Rectangle,
};

use crate::{
    SysError,
    display::{Display, SCREEN_HEIGHT, SCREEN_WIDTH},
};

pub struct FrameBuffer {
    pixels: Vec<u16>,
    size: Size,
    dirty_rows: Option<Range<usize>>,
}

impl FrameBuffer {
    pub fn new(size: Size) -> Result<Self, SysError> {
        if size.width == 0
            || size.height == 0
            || size.width > SCREEN_WIDTH as u32
            || size.height > SCREEN_HEIGHT as u32
        {
            return Err(SysError::InvalidArgument);
        }
        let count = size.width as usize * size.height as usize;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| SysError::NoMemory)?;
        pixels.resize(count, 0);
        Ok(Self {
            pixels,
            size,
            dirty_rows: Some(0..size.height as usize),
        })
    }

    pub fn present(&mut self, display: &mut Display, origin: Point) -> Result<(), SysError> {
        if origin.x < 0
            || origin.y < 0
            || i64::from(origin.x) + i64::from(self.size.width) > SCREEN_WIDTH as i64
            || i64::from(origin.y) + i64::from(self.size.height) > SCREEN_HEIGHT as i64
        {
            return Err(SysError::InvalidArgument);
        }
        if let Some(rows) = self.dirty_rows.clone() {
            let width = self.size.width as usize;
            display.blit(
                Rectangle::new(
                    origin + Point::new(0, rows.start as i32),
                    Size::new(self.size.width, (rows.end - rows.start) as u32),
                ),
                &self.pixels[rows.start * width..rows.end * width],
            )?;
            self.dirty_rows = None;
        }
        Ok(())
    }

    fn mark_rows(&mut self, rows: Range<usize>) {
        match &mut self.dirty_rows {
            Some(dirty) => {
                dirty.start = dirty.start.min(rows.start);
                dirty.end = dirty.end.max(rows.end);
            }
            None => self.dirty_rows = Some(rows),
        }
    }
}

impl OriginDimensions for FrameBuffer {
    fn size(&self) -> Size {
        self.size
    }
}

impl DrawTarget for FrameBuffer {
    type Color = Rgb565;
    type Error = ();

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if point.x < 0
                || point.y < 0
                || point.x >= self.size.width as i32
                || point.y >= self.size.height as i32
            {
                continue;
            }
            let row = point.y as usize;
            let index = row * self.size.width as usize + point.x as usize;
            let raw = RawU16::from(color).into_inner();
            if self.pixels[index] != raw {
                self.pixels[index] = raw;
                self.mark_rows(row..row + 1);
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Self::Color) -> Result<(), Self::Error> {
        let Some((left, top, width, height)) = userlib_sys::clip_rect(
            area.top_left.x,
            area.top_left.y,
            area.size.width,
            area.size.height,
            self.size.width,
            self.size.height,
        ) else {
            return Ok(());
        };
        let raw = RawU16::from(color).into_inner();
        for row in top as usize..(top + height) as usize {
            let start = row * self.size.width as usize + left as usize;
            let pixels = &mut self.pixels[start..start + width as usize];
            if pixels.iter().any(|pixel| *pixel != raw) {
                pixels.fill(raw);
                self.mark_rows(row..row + 1);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clips_draws_and_preserves_skipped_frame_changes() {
        let mut buffer = FrameBuffer::new(Size::new(4, 3)).unwrap();
        buffer.dirty_rows = None;
        buffer
            .fill_solid(
                &Rectangle::new(Point::new(-1, -1), Size::new(3, 3)),
                Rgb565::RED,
            )
            .unwrap();
        buffer
            .draw_iter([
                Pixel(Point::new(3, 2), Rgb565::GREEN),
                Pixel(Point::new(-1, 2), Rgb565::BLUE),
                Pixel(Point::new(4, 2), Rgb565::BLUE),
            ])
            .unwrap();
        assert_eq!(buffer.dirty_rows, Some(0..3));
        assert_eq!(buffer.pixels[0], RawU16::from(Rgb565::RED).into_inner());
        assert_eq!(buffer.pixels[11], RawU16::from(Rgb565::GREEN).into_inner());
        assert_eq!(buffer.pixels[2], 0);
        buffer.dirty_rows = None;
        buffer
            .draw_iter([Pixel(Point::new(3, 2), Rgb565::GREEN)])
            .unwrap();
        assert!(buffer.dirty_rows.is_none());
    }

    #[test]
    fn rejects_empty_or_oversized_buffers() {
        for size in [
            Size::new(0, 1),
            Size::new(1, 0),
            Size::new(321, 1),
            Size::new(1, u32::MAX),
        ] {
            assert!(FrameBuffer::new(size).is_err());
        }
    }
}
