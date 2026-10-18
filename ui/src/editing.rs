//! Allocation-free bounded editing. UTF-8 boundaries and capacity are checked
//! before mutation; applications retain their own permission/save protocols.
use crate::Error;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub caret: usize,
}
impl Selection {
    pub const fn at(end: usize) -> Self {
        Self {
            anchor: end,
            caret: end,
        }
    }
    pub fn range(self) -> core::ops::Range<usize> {
        self.anchor.min(self.caret)..self.anchor.max(self.caret)
    }
}
pub fn replace_selection(
    bytes: &mut [u8],
    len: usize,
    selection: Selection,
    input: &str,
) -> Result<(usize, Selection), Error> {
    let text =
        core::str::from_utf8(bytes.get(..len).ok_or(Error::Text)?).map_err(|_| Error::Text)?;
    let range = selection.range();
    if !text.is_char_boundary(range.start) || !text.is_char_boundary(range.end) {
        return Err(Error::Text);
    }
    let next = len
        .checked_sub(range.len())
        .and_then(|n| n.checked_add(input.len()))
        .filter(|n| *n <= bytes.len())
        .ok_or(Error::Limit)?;
    bytes.copy_within(range.end..len, range.start + input.len());
    bytes[range.start..range.start + input.len()].copy_from_slice(input.as_bytes());
    bytes[next..].fill(0);
    Ok((next, Selection::at(range.start + input.len())))
}
pub fn append(bytes: &mut [u8], len: usize, input: &str) -> Result<usize, Error> {
    replace_selection(bytes, len, Selection::at(len), input).map(|r| r.0)
}
pub fn replace(bytes: &mut [u8], len: usize, input: &str) -> Result<usize, Error> {
    replace_selection(
        bytes,
        len,
        Selection {
            anchor: 0,
            caret: len,
        },
        input,
    )
    .map(|r| r.0)
}
pub fn backspace(bytes: &mut [u8], len: usize) -> Result<usize, Error> {
    let text =
        core::str::from_utf8(bytes.get(..len).ok_or(Error::Text)?).map_err(|_| Error::Text)?;
    let previous = text.char_indices().next_back().map_or(0, |r| r.0);
    replace_selection(
        bytes,
        len,
        Selection {
            anchor: previous,
            caret: len,
        },
        "",
    )
    .map(|r| r.0)
}
