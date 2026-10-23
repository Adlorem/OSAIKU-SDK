//! Fixed-cell virtual grids over the same bounded row planner as virtual lists.
//! Hosts bind only `Range::first..first + count` into recycled cells and retain
//! stable item identities alongside the presented revision for input validation.
use crate::{layout::Rect, virtual_list, Error};

#[derive(Clone, Copy)]
pub struct Config {
    pub columns: u16,
    pub row_height: u16,
    pub gap: u16,
    pub total: u32,
}
#[derive(Clone, Copy)]
pub struct Range {
    pub first: u32,
    pub count: u32,
    pub scroll: u32,
    pub extent: u32,
}
impl Config {
    pub fn range(self, scroll: u32, height: u32, capacity: usize) -> Result<Range, Error> {
        if self.columns == 0 || self.columns > 32 || self.gap >= self.row_height {
            return Err(Error::Value);
        }
        let columns = u32::from(self.columns);
        let rows = self.total.div_ceil(columns);
        let range = virtual_list::range(
            virtual_list::Config {
                source: 1,
                item_height: self.row_height,
                overscan: 0,
                total: rows,
                first: 0,
            },
            scroll,
            height,
            capacity / columns as usize,
        )?;
        let first = range.first * columns;
        Ok(Range {
            first,
            count: (range.count * columns).min(self.total - first),
            scroll: range.scroll,
            extent: rows * u32::from(self.row_height),
        })
    }
    /// A recycled cell's full bounds; paint and hit testing both clip to viewport.
    pub fn cell(self, range: Range, viewport: Rect, slot: u32) -> Option<Rect> {
        if slot >= range.count
            || slot >= self.total
            || self.columns == 0
            || self.columns > 32
            || self.row_height == 0
            || self.row_height > 4096
            || self.gap >= self.row_height
        {
            return None;
        }
        let columns = u32::from(self.columns);
        let gap = u32::from(self.gap);
        let width = viewport.width.checked_sub((columns - 1) * gap)? / columns;
        Some(Rect {
            x: viewport.x.checked_add(
                i32::try_from((slot % columns).checked_mul(width.checked_add(gap)?)?).ok()?,
            )?,
            y: viewport
                .y
                .checked_add(
                    i32::try_from((slot / columns).checked_mul(u32::from(self.row_height))?)
                        .ok()?,
                )?
                .checked_sub((range.scroll % u32::from(self.row_height)) as i32)?,
            width,
            height: u32::from(self.row_height) - gap,
        })
    }
}
