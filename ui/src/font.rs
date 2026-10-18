//! Two typefaces, including the Polish repertoire:
//! - UI text (`ui_text_*`): proportional Inter, one pre-rasterized table per
//!   admitted font size; licence ../licenses/Inter-OFL.txt.
//! - Native cell grid (`ui_font_*`): fixed DejaVu Sans Mono cells for the
//!   pinned native presentation path; licence ../licenses/DejaVu.txt.
use crate::font_atlas::{UI_FONT_ATLAS_CODEPOINTS, UI_FONT_ATLAS_EMPHASIS, UI_FONT_ATLAS_PRIMARY};
use crate::text_atlas::{
    UI_TEXT_ATLAS_CODEPOINTS, UI_TEXT_ATLAS_COVERAGE, UI_TEXT_ATLAS_GLYPHS,
    UI_TEXT_ATLAS_GLYPH_COUNT, UI_TEXT_SIZES,
};
pub const UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_HEIGHT: u8 = 16;
pub const UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_WIDTH: u32 = 7;
pub const UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_HEIGHT: u8 = 20;
pub const UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_WIDTH: u32 = 9;
// Bytes per glyph in each table. The tables declare their lengths as the glyph
// count times these same products, so a table that disagrees with its cell --
// a glyph that would read its neighbour's pixels -- does not compile.
const PRIMARY_STRIDE: usize = UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_WIDTH as usize
    * UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_HEIGHT as usize;
const EMPHASIS_STRIDE: usize = UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_WIDTH as usize
    * UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_HEIGHT as usize;

/// One glyph's coverage and the cell it was rasterized for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiFontGlyph {
    coverage: &'static [u8],
    width: u32,
    height: u8,
}

impl UiFontGlyph {
    /// Coverage, row-major, `width * height` bytes.
    pub const fn coverage(&self) -> &'static [u8] {
        self.coverage
    }

    /// Cell width in pixels, which is also the glyph's advance.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Cell height in pixels.
    pub const fn height(&self) -> u8 {
        self.height
    }
}

/// The cell width for a raster height, or `None` for a height with no cell.
pub const fn ui_font_raster_width(raster_height: u8) -> Option<u32> {
    match raster_height {
        UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_HEIGHT => {
            Some(UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_WIDTH)
        }
        UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_HEIGHT => {
            Some(UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_WIDTH)
        }
        _ => None,
    }
}

/// One UI-text glyph at one admitted size: its inked coverage and box,
/// relative to the pen position and the top of the line box.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiTextGlyph {
    coverage: &'static [u8],
    left: i8,
    top: u8,
    width: u8,
    height: u8,
    advance: u16,
}

impl UiTextGlyph {
    /// Coverage 0..=255 of inked pixel (x, y), row-major from the box's top
    /// left. Stored as 16 levels, two pixels per byte, high nibble first.
    pub fn alpha(&self, x: u32, y: u32) -> u8 {
        if x >= self.width() || y >= self.height() {
            return 0;
        }
        let pixel = (y * self.width() + x) as usize;
        let byte = self.coverage[pixel / 2];
        let level = if pixel % 2 == 0 {
            byte >> 4
        } else {
            byte & 0x0f
        };
        level * 17
    }
    /// Horizontal offset of the inked box from the pen position.
    pub const fn left(&self) -> i32 {
        self.left as i32
    }
    /// First inked row below the top of the line box.
    pub const fn top(&self) -> u32 {
        self.top as u32
    }
    pub const fn width(&self) -> u32 {
        self.width as u32
    }
    pub const fn height(&self) -> u32 {
        self.height as u32
    }
    /// Pen advance in 1/64 px.
    pub const fn advance(&self) -> u32 {
        self.advance as u32
    }
}

/// Whether CSS `font-size` has a UI-text table. Only these sizes are admitted:
/// text is never scaled at runtime.
pub const fn ui_text_size_supported(size: u16) -> bool {
    let mut i = 0;
    while i < UI_TEXT_SIZES.len() {
        if UI_TEXT_SIZES[i] == size {
            return true;
        }
        i += 1;
    }
    false
}

/// The glyph for `character` at `size`. A character the atlas lacks is drawn
/// as '?'; an unadmitted size has no glyphs.
pub fn ui_text_glyph(character: char, size: u16) -> Option<UiTextGlyph> {
    let table = UI_TEXT_SIZES.iter().position(|s| *s == size)?;
    let index = UI_TEXT_ATLAS_CODEPOINTS
        .binary_search(&(character as u32))
        .or_else(|_| UI_TEXT_ATLAS_CODEPOINTS.binary_search(&('?' as u32)))
        .ok()?;
    let (offset, left, top, width, height, advance) =
        *UI_TEXT_ATLAS_GLYPHS.get(table * UI_TEXT_ATLAS_GLYPH_COUNT + index)?;
    let start = offset as usize;
    let end = start + (width as usize * height as usize).div_ceil(2);
    Some(UiTextGlyph {
        coverage: UI_TEXT_ATLAS_COVERAGE.get(start..end)?,
        left,
        top,
        width,
        height,
        advance,
    })
}

/// One line of UI text: each character and its glyph with the x offset of its
/// pen position, in whole pixels from the line start. Pens advance in 1/64 px
/// and round to the nearest pixel, so spacing does not drift along a line.
/// Paint adapters and measurement share this placement.
pub fn ui_text_run(text: &str, size: u16) -> impl Iterator<Item = (i32, char, UiTextGlyph)> + '_ {
    let mut pen = 0u32;
    text.chars().filter_map(move |character| {
        let glyph = ui_text_glyph(character, size)?;
        let x = (pen + 32) >> 6;
        pen = pen.saturating_add(glyph.advance());
        Some((x as i32, character, glyph))
    })
}

/// Width of a single-line label at `size`: its rounded total advance.
pub fn ui_font_text_width(text: &str, size: u16) -> u32 {
    let total = text
        .chars()
        .filter_map(|character| ui_text_glyph(character, size))
        .fold(0u32, |pen, glyph| pen.saturating_add(glyph.advance()));
    (total + 32) >> 6
}

/// The glyph for `character` at `raster_height`, or `None` when the atlas has
/// no such character or no such cell.
pub fn ui_font_glyph(character: char, raster_height: u8) -> Option<UiFontGlyph> {
    let index = UI_FONT_ATLAS_CODEPOINTS
        .binary_search(&(character as u32))
        .ok()?;
    let (table, width, stride) = match raster_height {
        UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_HEIGHT => (
            &UI_FONT_ATLAS_PRIMARY[..],
            UI_RESPONSIVE_LAYOUT_FONT_PRIMARY_RASTER_WIDTH,
            PRIMARY_STRIDE,
        ),
        UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_HEIGHT => (
            &UI_FONT_ATLAS_EMPHASIS[..],
            UI_RESPONSIVE_LAYOUT_FONT_EMPHASIS_RASTER_WIDTH,
            EMPHASIS_STRIDE,
        ),
        _ => return None,
    };
    let start = index.checked_mul(stride)?;
    let end = start.checked_add(stride)?;
    Some(UiFontGlyph {
        coverage: table.get(start..end)?,
        width,
        height: raster_height,
    })
}
