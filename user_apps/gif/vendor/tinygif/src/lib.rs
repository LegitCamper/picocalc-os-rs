//! tinygif - A tiny GIF decoder for embedded systems.

#![no_std]

extern crate alloc;

use core::fmt::{self, Debug};
use core::marker::PhantomData;

use embedded_graphics::prelude::{
    DrawTarget, DrawTargetExt, ImageDrawable, OriginDimensions, Point, Size, Transform,
};
use embedded_graphics::Pixel;
use embedded_graphics::{pixelcolor::Rgb888, prelude::PixelColor};
use parser::eat_len_prefixed_subblocks;

use crate::parser::{le_u16, take, take1, take_slice};

mod bitstream;
pub mod lzw;
mod parser;

/// Len byte prefixed raw bytes, as used in GIFs.
struct LenPrefixRawDataView<'a> {
    remains: &'a [u8],
    current_block: &'a [u8],
    cursor: u8,
}

impl<'a> LenPrefixRawDataView<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        let len = data[0] as usize;
        Self {
            remains: &data[1 + len..],
            current_block: &data[1..1 + len],
            cursor: 0,
        }
    }

    #[inline]
    fn shift_cursor(&mut self) {
        if self.current_block.is_empty() {
            // nop
        } else if self.cursor < self.current_block.len() as u8 - 1 {
            self.cursor += 1;
        } else {
            self.cursor = 0;
            self.shift_next_block();
        }
    }

    // leave cursor untouched
    #[inline]
    fn shift_next_block(&mut self) {
        if self.current_block.is_empty() {
            // no more blocks
            return;
        }
        let len = self.remains[0] as usize;
        if len == 0 {
            self.remains = &[];
            self.current_block = &[];
        } else {
            self.current_block = &self.remains[1..1 + len];
            self.remains = &self.remains[1 + len..];
        }
    }
}

impl Iterator for LenPrefixRawDataView<'_> {
    type Item = u8;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_block.is_empty() {
            return None;
        }

        let current = self.current_block[self.cursor as usize];
        self.shift_cursor();
        Some(current)
    }
}

#[non_exhaustive]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Version {
    V87a,
    V89a,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Header {
    version: Version,
    pub width: u16,
    pub height: u16,
    has_global_color_table: bool,
    color_resolution: u8, // 3 bits
    // _is_sorted: bool,
    pub bg_color_index: u8,
    // _pixel_aspect_ratio: u8
}

impl Header {
    pub fn parse(input: &[u8]) -> Result<(&[u8], (Header, Option<ColorTable<'_>>)), ParseError> {
        let (input, magic) = take::<3>(input)?;

        if &magic != b"GIF" {
            return Err(ParseError::InvalidFileSignature(magic));
        }

        let (input, ver) = take::<3>(input)?;
        let version = if &ver == b"87a" {
            Version::V87a
        } else if &ver == b"89a" {
            Version::V89a
        } else {
            return Err(ParseError::InvalidFileSignature(magic));
        };

        let (intput, screen_width) = le_u16(input)?;
        let (intput, screen_height) = le_u16(intput)?;
        if screen_width == 0 || screen_height == 0 {
            return Err(ParseError::InvalidImageDimensions);
        }

        let (input, flags) = take1(intput)?;
        let has_global_color_table = flags & 0b1000_0000 != 0;
        let global_color_table_size = if has_global_color_table {
            2_usize.pow(((flags & 0b0000_0111) + 1) as u32)
        } else {
            0
        };
        let color_resolution = (flags & 0b0111_0000) >> 4;
        let _is_sorted = flags & 0b0000_1000 != 0;

        let (input, bg_color_index) = take1(input)?;
        let (input, _pixel_aspect_ratio) = take1(input)?;

        let (input, color_table) = if global_color_table_size > 0 {
            // Each color table entry is 3 bytes long
            let (input, table) = take_slice(input, global_color_table_size * 3)?;
            (input, Some(ColorTable::new(table)))
        } else {
            (input, None)
        };

        Ok((
            input,
            (
                Header {
                    version,
                    width: screen_width,
                    height: screen_height,
                    has_global_color_table,
                    color_resolution,
                    bg_color_index,
                },
                color_table,
            ),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ColorTable<'a> {
    data: &'a [u8],
}

impl<'a> ColorTable<'a> {
    pub(crate) const fn new(data: &'a [u8]) -> Self {
        Self { data }
    }

    /// Returns the number of entries.
    pub const fn len(&self) -> usize {
        self.data.len() / 3
    }

    /// Returns a color table entry.
    ///
    /// `None` is returned if `index` is out of bounds.
    pub fn get(&self, index: u8) -> Option<Rgb888> {
        let base = 3 * (index as usize);
        if base >= self.data.len() {
            return None;
        }

        Some(Rgb888::new(
            self.data[base],
            self.data[base + 1],
            self.data[base + 2],
        ))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct RawGif<'a> {
    /// Image header.
    header: Header,

    global_color_table: Option<ColorTable<'a>>,

    /// Image data.
    raw_block_data: &'a [u8],
}

impl<'a> RawGif<'a> {
    fn from_slice(bytes: &'a [u8]) -> Result<Self, ParseError> {
        let (remaining, (header, color_table)) = Header::parse(bytes)?;
        let mut input = remaining;
        let mut images = 0;
        loop {
            let (rest, segment) = Segment::parse(input)?;
            match segment {
                Segment::Image(image) => {
                    if u32::from(image.left) + u32::from(image.width) > u32::from(header.width)
                        || u32::from(image.top) + u32::from(image.height) > u32::from(header.height)
                        || image.local_color_table.or(color_table).is_none()
                    {
                        return Err(ParseError::InvalidImageDimensions);
                    }
                    images += 1;
                }
                Segment::Trailer => {
                    if images == 0 {
                        return Err(ParseError::InvalidImageDimensions);
                    }
                    break;
                }
                _ => (),
            }
            input = rest;
        }

        Ok(Self {
            header,
            global_color_table: color_table,
            raw_block_data: remaining,
        })
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct GraphicControl {
    pub disposal_method: u8,
    pub is_transparent: bool,
    pub transparent_color_index: u8,
    // centisecond
    pub delay_centis: u16,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]

pub struct ImageBlock<'a> {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
    pub is_interlaced: bool,
    pub lzw_min_code_size: u8,
    local_color_table: Option<ColorTable<'a>>,
    image_data: &'a [u8],
}

impl<'a> ImageBlock<'a> {
    // parse after 0x2c separator
    pub fn parse(input: &'a [u8]) -> Result<(&'a [u8], Self), ParseError> {
        let (input, left) = le_u16(input)?;
        let (input, top) = le_u16(input)?;
        let (input, width) = le_u16(input)?;
        let (input, height) = le_u16(input)?;
        if width == 0 || height == 0 {
            return Err(ParseError::InvalidImageDimensions);
        }
        let (input, flags) = take1(input)?;
        let is_interlaced = flags & 0b0100_0000 != 0;
        let has_local_color_table = flags & 0b1000_0000 != 0;
        let local_color_table_size = if has_local_color_table {
            2_usize.pow(((flags & 0b0000_0111) + 1) as u32)
        } else {
            0
        };

        let (input, local_color_table) = if local_color_table_size > 0 {
            // Each color table entry is 4 bytes long
            let (input, table) = take_slice(input, local_color_table_size * 3)?;
            (input, Some(ColorTable::new(table)))
        } else {
            (input, None)
        };

        let (input, lzw_min_code_size) = take1(input)?;
        if !(2..=8).contains(&lzw_min_code_size) {
            return Err(ParseError::InvalidByte);
        }

        let mut input0 = input;
        let mut n = 1;
        loop {
            let (input, block_size) = take1(input0)?;
            if block_size == 0 {
                input0 = input;
                break;
            }
            let (input, _) = take_slice(input, block_size as usize)?;
            n += block_size as usize + 1;
            input0 = input;
        }

        Ok((
            input0,
            Self {
                left,
                top,
                width,
                height,
                is_interlaced,
                lzw_min_code_size,
                local_color_table,
                image_data: &input[..n],
            },
        ))
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum ExtensionBlock<'a> {
    GraphicControl(GraphicControl),
    Comment(&'a [u8]),
    // TODO: handle PlainTextExtension
    PlainText,
    NetscapeApplication { repetitions: u16 },
    Application, // ignore content
    Unknown(u8, &'a [u8]),
}

impl<'a> ExtensionBlock<'a> {
    pub fn parse(input: &'a [u8]) -> Result<(&'a [u8], Self), ParseError> {
        let (input, ext_label) = take1(input)?;
        match ext_label {
            0xff => {
                let (input, block_size_1) = take1(input)?;
                let (input, app_id) = take::<8>(input)?;
                let (input, app_auth_code) = take::<3>(input)?;
                if block_size_1 == 11 && &app_id == b"NETSCAPE" && &app_auth_code == b"2.0" {
                    let (input, block_size_2) = take1(input)?;
                    if block_size_2 == 3 {
                        let (input, always_one) = take1(input)?;
                        if always_one == 1 {
                            let (input, repetitions) = le_u16(input)?;
                            let (input, eob) = take1(input)?;
                            if eob == 0 {
                                return Ok((
                                    input,
                                    ExtensionBlock::NetscapeApplication { repetitions },
                                ));
                            }
                        }
                    }
                }
                let mut input0 = input;
                loop {
                    let (input, block_size_2) = take1(input0)?;
                    if block_size_2 == 0 {
                        input0 = input;
                        break;
                    }
                    let (input, _data) = take_slice(input, block_size_2 as usize)?;
                    input0 = input;
                }
                Ok((input0, ExtensionBlock::Application))
            }
            0xfe => {
                // Comment Extension
                let mut input0 = input;
                let mut size = 0;
                loop {
                    let (input, block_size) = take1(input0)?;
                    size += 1;
                    if block_size == 0 {
                        input0 = input;
                        break;
                    }
                    let (input, _data) = take_slice(input, block_size as usize)?;
                    input0 = input;
                    size += block_size as usize;
                }
                Ok((input0, ExtensionBlock::Comment(&input[..size])))
            }
            0xf9 => {
                // Graphic Control Extension
                let (input, block_size) = take1(input)?; // 4
                if block_size != 4 {
                    return Err(ParseError::InvalidConstSizeBytes); // invalid block size
                }
                let (input, flags) = take1(input)?;
                let is_transparent = flags & 0b0000_0001 != 0;
                let (input, delay_centis) = le_u16(input)?;
                let (input, transparent_color_index) = take1(input)?;
                let (input, block_terminator) = take1(input)?;
                if block_terminator != 0 {
                    return Err(ParseError::InvalidByte);
                }

                Ok((
                    input,
                    ExtensionBlock::GraphicControl(GraphicControl {
                        disposal_method: (flags >> 2) & 7,
                        is_transparent,
                        transparent_color_index,
                        delay_centis,
                    }),
                ))
            }
            0x01 => {
                let (input, _) = take::<13>(input)?;
                let input = eat_len_prefixed_subblocks(input)?;
                Ok((input, ExtensionBlock::PlainText))
            }
            _ => Err(ParseError::InvalidExtensionLabel),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Segment<'a> {
    Image(ImageBlock<'a>),
    Extension(ExtensionBlock<'a>),
    // 0x3b
    Trailer,
}

impl<'a> Segment<'a> {
    pub fn parse(input: &'a [u8]) -> Result<(&'a [u8], Self), ParseError> {
        let (input, ext_magic) = take1(input)?;

        if ext_magic == 0x21 {
            let (input, ext) = ExtensionBlock::parse(input)?;
            Ok((input, Segment::Extension(ext)))
        } else if ext_magic == 0x2c {
            // Image Block
            let (input, image_block) = ImageBlock::parse(input)?;
            Ok((input, Segment::Image(image_block)))
        } else if ext_magic == 0x3b {
            if input.is_empty() {
                Ok((input, Segment::Trailer))
            } else {
                Err(ParseError::JunkAfterTrailerByte)
            }
        } else {
            return Err(ParseError::InvalidByte);
        }
    }

    fn skip_to_next_graphic_control(mut input: &[u8]) -> Result<&[u8], ParseError> {
        loop {
            let (input0, ext_magic) = take1(input)?;
            if ext_magic == 0x21 {
                let (input1, label) = take1(input0)?;
                if label == 0xF9 {
                    return Ok(input);
                } else if label == 0xFF {
                    // Application Extension
                    input = eat_len_prefixed_subblocks(input1)?;
                } else if label == 0x01 {
                    // Plain Text Extension
                    // Ref: https://www.w3.org/Graphics/GIF/spec-gif89a.txt
                    let (input2, _) = take::<13>(input1)?;
                    input = eat_len_prefixed_subblocks(input2)?;
                } else if label == 0xFE {
                    // Comment Extension
                    input = eat_len_prefixed_subblocks(input1)?;
                } else {
                    return Err(ParseError::InvalidExtensionLabel);
                }
            } else if ext_magic == 0x2c {
                // Image Descriptor
                // parse image, optional local color table
                let (input1, _) = ImageBlock::parse(input0)?;
                input = input1;
            } else if ext_magic == 0x3b {
                // Trailer
                return Ok(&[]);
            } else if ext_magic == 0x00 {
                // Block Terminator
                return Ok(input0);
            } else {
                return Err(ParseError::InvalidByte);
            }
        }
    }

    pub const fn type_name(&self) -> &'static str {
        match self {
            Segment::Image(_) => "Image",
            Segment::Extension(_) => "Extension",
            Segment::Trailer => "Trailer",
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Gif<'a, C = Rgb888> {
    raw_gif: RawGif<'a>,
    color_type: PhantomData<C>,
}

impl<'a, C> Gif<'a, C> {
    pub fn from_slice(input: &'a [u8]) -> Result<Self, ParseError> {
        let raw_gif = RawGif::from_slice(input)?;
        Ok(Self {
            raw_gif,
            color_type: PhantomData,
        })
    }

    pub fn frames(&'a self) -> FrameIterator<'a, C> {
        FrameIterator::new(self)
    }

    pub fn width(&self) -> u16 {
        self.raw_gif.header.width
    }

    pub fn height(&self) -> u16 {
        self.raw_gif.header.height
    }
}

pub struct FrameIterator<'a, C> {
    gif: &'a Gif<'a, C>,
    frame_index: usize,
    remain_raw_data: &'a [u8],
}

impl<'a, C> FrameIterator<'a, C> {
    fn new(gif: &'a Gif<'a, C>) -> Self {
        Self {
            gif,
            frame_index: 0,
            remain_raw_data: gif.raw_gif.raw_block_data,
        }
    }
}

impl<'a, C: PixelColor> Iterator for FrameIterator<'a, C> {
    type Item = Frame<'a, C>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remain_raw_data.is_empty() {
            return None;
        }

        let input = self.remain_raw_data;
        let input0 = Segment::skip_to_next_graphic_control(input).ok()?;

        let (input00, seg) = Segment::parse(input0).ok()?;
        self.remain_raw_data = input00;

        if let Segment::Extension(ExtensionBlock::GraphicControl(ctrl)) = seg {
            let frame = Frame {
                disposal_method: ctrl.disposal_method,
                delay_centis: ctrl.delay_centis,
                is_transparent: ctrl.is_transparent,
                transparent_color_index: ctrl.transparent_color_index,
                global_color_table: self.gif.raw_gif.global_color_table.clone(),
                header: &self.gif.raw_gif.header,
                raw_data: input00,
                frame_index: self.frame_index,
                transform: None,
                _marker: PhantomData,
            };
            self.frame_index += 1;
            return Some(frame);
        } else {
            None
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Frame<'a, C> {
    pub disposal_method: u8,
    pub delay_centis: u16,
    pub is_transparent: bool,
    pub transparent_color_index: u8,
    global_color_table: Option<ColorTable<'a>>,
    header: &'a Header,
    raw_data: &'a [u8],
    frame_index: usize,
    transform: Option<Point>,
    _marker: PhantomData<C>,
}

impl<'a, C> OriginDimensions for Frame<'a, C> {
    fn size(&self) -> Size {
        Size::new(self.header.width as _, self.header.height as _)
    }
}

impl<'a, C> Transform for Frame<'a, C>
where
    C: PixelColor + From<Rgb888>,
{
    fn translate(&self, by: Point) -> Self {
        let mut frame = self.clone();
        frame.transform = Some(by);
        frame
    }

    fn translate_mut(&mut self, by: Point) -> &mut Self {
        self.transform = Some(by);
        self
    }
}

#[derive(Debug)]
pub enum DrawError<Error> {
    Parse(ParseError),
    Target(Error),
}

impl<'a, C> Frame<'a, C>
where
    C: PixelColor + From<Rgb888>,
{
    /// Whether this frame replaces the entire canvas without needing its old pixels.
    pub fn is_self_contained(&self) -> bool {
        if self.is_transparent || self.disposal_method > 1 {
            return false;
        }
        matches!(Segment::parse(self.raw_data), Ok((_, Segment::Image(image)))
            if image.left == 0 && image.top == 0
                && image.width == self.header.width && image.height == self.header.height)
    }

    /// Draws a frame while preserving malformed-input and target errors separately.
    pub fn draw_checked<D>(&self, target: &mut D) -> Result<(), DrawError<D::Error>>
    where
        D: DrawTarget<Color = C>,
    {
        let mut input = self.raw_data;
        loop {
            let (rest, segment) = Segment::parse(input).map_err(DrawError::Parse)?;
            input = rest;
            match segment {
                Segment::Extension(ExtensionBlock::GraphicControl(_)) | Segment::Trailer => break,
                Segment::Image(image) => {
                    let palette = image
                        .local_color_table
                        .or(self.global_color_table)
                        .ok_or(DrawError::Parse(ParseError::InvalidByte))?;
                    let transparent = self.is_transparent.then_some(self.transparent_color_index);
                    let colors: [Option<C>; 256] =
                        core::array::from_fn(|index| palette.get(index as u8).map(C::from));
                    let transform = self.transform.unwrap_or(Point::zero());
                    let left = i32::from(image.left)
                        .checked_add(transform.x)
                        .ok_or(DrawError::Parse(ParseError::InvalidImageDimensions))?;
                    let top = i32::from(image.top)
                        .checked_add(transform.y)
                        .ok_or(DrawError::Parse(ParseError::InvalidImageDimensions))?;
                    left.checked_add(i32::from(image.width) - 1)
                        .ok_or(DrawError::Parse(ParseError::InvalidImageDimensions))?;
                    top.checked_add(i32::from(image.height) - 1)
                        .ok_or(DrawError::Parse(ParseError::InvalidImageDimensions))?;
                    let expected = u32::from(image.width) * u32::from(image.height);
                    let mut count = 0_u32;
                    let mut column = 0_u16;
                    let mut source_row = 0_u32;
                    let mut row = 0_u32;
                    let raw = LenPrefixRawDataView::new(image.image_data);
                    let mut decoder = lzw::Decoder::new(raw, image.lzw_min_code_size)
                        .map_err(DrawError::Parse)?;
                    while let Some(decoded) = decoder.decode_next().map_err(DrawError::Parse)? {
                        if decoded.len() as u32 > expected - count
                            || decoded.iter().any(|&index| {
                                transparent != Some(index) && colors[index as usize].is_none()
                            })
                        {
                            return Err(DrawError::Parse(ParseError::InvalidByte));
                        }
                        target
                            .draw_iter(decoded.iter().filter_map(|&index| {
                                let point = Point::new(left + i32::from(column), top + row as i32);
                                column += 1;
                                if column == image.width {
                                    column = 0;
                                    source_row += 1;
                                    row = if image.is_interlaced {
                                        interlaced_row(source_row, u32::from(image.height))
                                    } else {
                                        source_row
                                    };
                                }
                                if transparent == Some(index) {
                                    None
                                } else {
                                    colors[index as usize].map(|color| Pixel(point, color))
                                }
                            }))
                            .map_err(DrawError::Target)?;
                        count += decoded.len() as u32;
                    }
                    if count != expected {
                        return Err(DrawError::Parse(ParseError::UnexpectedEndOfFile));
                    }
                }
                _ => (),
            }
        }
        Ok(())
    }
}

fn interlaced_row(mut row: u32, height: u32) -> u32 {
    for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
        let count = height.saturating_sub(start).div_ceil(step);
        if row < count {
            return start + row * step;
        }
        row -= count;
    }
    height
}

impl<'a, C> ImageDrawable for Frame<'a, C>
where
    C: PixelColor + From<Rgb888>,
{
    type Color = C;

    fn draw<D>(&self, target: &mut D) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        match self.draw_checked(target) {
            Ok(()) | Err(DrawError::Parse(_)) => Ok(()),
            Err(DrawError::Target(error)) => Err(error),
        }
    }

    fn draw_sub_image<D>(
        &self,
        target: &mut D,
        area: &embedded_graphics::primitives::Rectangle,
    ) -> Result<(), D::Error>
    where
        D: DrawTarget<Color = Self::Color>,
    {
        self.draw(&mut target.clipped(area))
    }
}

impl<C> fmt::Debug for Frame<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Frame")
            .field("frame_index", &self.frame_index)
            .field("delay_centis", &self.delay_centis)
            .field("is_transparent", &self.is_transparent)
            .field("transparent_color_index", &self.transparent_color_index)
            .field("len(remain_data)", &self.raw_data.len())
            .finish()
    }
}

#[cfg(feature = "defmt")]
impl<C> defmt::Format for Frame<'_, C> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(
            f,
            "Frame {{ frame_index: {}, delay_centis: {} remain: {}}}",
            self.frame_index,
            self.delay_centis,
            self.raw_data.len()
        );
    }
}

/// Parse error.
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum ParseError {
    OutOfMemory,
    /// The image uses an unsupported bit depth.
    UnsupportedBpp(u16),

    /// Unexpected end of file.
    UnexpectedEndOfFile,

    /// Invalid file signatures.
    InvalidFileSignature([u8; 3]),

    /// Unsupported compression method.
    UnsupportedCompressionMethod(u32),

    /// Unsupported header length.
    UnsupportedHeaderLength(u32),

    /// Unsupported channel masks.
    UnsupportedChannelMasks,

    /// Invalid image dimensions.
    InvalidImageDimensions,

    InvalidByte,

    JunkAfterTrailerByte,

    /// Current size bytes should be a constant.
    InvalidConstSizeBytes,

    InvalidExtensionLabel,
}

#[cfg(test)]
mod tests {
    use super::*;
    use embedded_graphics::{mock_display::MockDisplay, pixelcolor::Rgb565, prelude::RgbColor};

    fn fixture() -> [u8; 43] {
        [
            b'G', b'I', b'F', b'8', b'9', b'a', 1, 0, 1, 0, 0x80, 0, 0, 0, 0, 0, 255, 255, 255,
            0x21, 0xf9, 4, 0, 1, 0, 0, 0, 0x2c, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 0x01, 0,
            0x3b,
        ]
    }

    #[test]
    fn valid_frame_retains_playback() {
        let bytes = fixture();
        let gif = Gif::<Rgb888>::from_slice(&bytes).unwrap();
        let frame = gif.frames().next().unwrap();
        let mut display = MockDisplay::new();
        frame.draw_checked(&mut display).unwrap();
        assert_eq!(display.get_pixel(Point::zero()), Some(Rgb888::BLACK));
        assert_eq!(frame.delay_centis, 1);
        assert!(frame.is_self_contained());
    }

    #[test]
    fn decoded_coordinates_cross_chunks_rows_and_transparent_pixels() {
        for interlaced in [false, true] {
            for transparent in [false, true] {
                let mut bytes = fixture()[..38].to_vec();
                bytes[6] = 3;
                bytes[8] = 5;
                bytes[22] = u8::from(transparent);
                bytes[25] = 1;
                bytes[32] = 3;
                bytes[34] = 5;
                bytes[36] = if interlaced { 0x40 } else { 0 };
                let codes = (0..15).flat_map(|pixel| [4_u8, pixel % 2]).chain([5]);
                let mut packed = alloc::vec::Vec::new();
                let mut pending = 0_u16;
                let mut bits = 0;
                for code in codes {
                    pending |= u16::from(code) << bits;
                    bits += 3;
                    if bits >= 8 {
                        packed.push(pending as u8);
                        pending >>= 8;
                        bits -= 8;
                    }
                }
                if bits != 0 {
                    packed.push(pending as u8);
                }
                bytes.push(packed.len() as u8);
                bytes.extend(packed);
                bytes.extend([0, 0x3b]);
                let gif = Gif::<Rgb565>::from_slice(&bytes).unwrap();
                let mut frame = gif.frames().next().unwrap();
                let mut display = MockDisplay::new();
                frame
                    .translate_mut(Point::new(1, 2))
                    .draw_checked(&mut display)
                    .unwrap();
                for index in 0..15 {
                    let row = if interlaced {
                        [0, 4, 2, 1, 3][index / 3]
                    } else {
                        index / 3
                    };
                    let expected = if index % 2 == 0 {
                        Some(Rgb565::BLACK)
                    } else if transparent {
                        None
                    } else {
                        Some(Rgb565::WHITE)
                    };
                    assert_eq!(
                        display.get_pixel(Point::new((index % 3 + 1) as i32, (row + 2) as i32)),
                        expected
                    );
                }
            }
        }
    }

    #[test]
    fn transparent_partial_and_restoring_frames_need_previous_pixels() {
        for flags in [1, 8, 12] {
            let mut bytes = fixture();
            bytes[22] = flags;
            let gif = Gif::<Rgb888>::from_slice(&bytes).unwrap();
            let frame = gif.frames().next().unwrap();
            assert!(!frame.is_self_contained());
            assert_eq!(frame.disposal_method, (flags >> 2) & 7);
        }
        let mut bytes = fixture();
        bytes[6] = 2;
        let gif = Gif::<Rgb888>::from_slice(&bytes).unwrap();
        assert!(!gif.frames().next().unwrap().is_self_contained());
    }

    #[test]
    fn decoding_unpresented_frame_preserves_transparent_delta_state() {
        let mut first = fixture();
        first[39] = 0x4c;
        let mut second = fixture();
        second[22] = 1;
        let mut bytes = first[..42].to_vec();
        bytes.extend_from_slice(&second[19..]);
        let gif = Gif::<Rgb888>::from_slice(&bytes).unwrap();
        let mut canvas = MockDisplay::new();
        canvas.set_allow_overdraw(true);
        let frames: alloc::vec::Vec<_> = gif.frames().collect();
        assert_eq!(frames.len(), 2);
        assert!(frames[0].is_self_contained());
        assert!(!frames[1].is_self_contained());
        for frame in frames {
            frame.draw_checked(&mut canvas).unwrap();
        }
        assert_eq!(canvas.get_pixel(Point::zero()), Some(Rgb888::WHITE));
    }

    #[test]
    fn truncated_and_invalid_metadata_return_errors() {
        let bytes = fixture();
        for length in 0..bytes.len() {
            assert!(Gif::<Rgb888>::from_slice(&bytes[..length]).is_err());
        }
        for (offset, value) in [(6, 0), (20, 0x55), (32, 0), (34, 0), (37, 255), (38, 255)] {
            let mut bytes = fixture();
            bytes[offset] = value;
            assert!(Gif::<Rgb888>::from_slice(&bytes).is_err());
        }
    }

    #[test]
    fn invalid_palette_codes_lzw_codes_and_missing_end_return_errors() {
        for code in [0x54, 0x7c, 0x04] {
            let mut bytes = fixture();
            bytes[39] = code;
            if code == 0x04 {
                bytes[40] = 0;
            }
            let gif = Gif::<Rgb888>::from_slice(&bytes).unwrap();
            let frame = gif.frames().next().unwrap();
            let mut display = MockDisplay::new();
            assert!(matches!(
                frame.draw_checked(&mut display),
                Err(DrawError::Parse(_))
            ));
        }
    }

    #[test]
    fn single_byte_mutations_never_panic_during_parse_or_render() {
        for offset in 0..43 {
            for value in 0..=255 {
                let mut bytes = fixture();
                bytes[offset] = value;
                if let Ok(gif) = Gif::<Rgb888>::from_slice(&bytes) {
                    for frame in gif.frames() {
                        let mut display = MockDisplay::new();
                        display.set_allow_overdraw(true);
                        display.set_allow_out_of_bounds_drawing(true);
                        let _ = frame.draw_checked(&mut display);
                    }
                }
            }
        }
    }

    #[test]
    fn interlaced_rows_follow_four_pass_order() {
        let rows: [u32; 8] = core::array::from_fn(|row| interlaced_row(row as u32, 8));
        assert_eq!(rows, [0, 4, 2, 6, 1, 3, 5, 7]);
    }
}
