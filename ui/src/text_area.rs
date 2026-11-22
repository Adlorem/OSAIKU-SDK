//! `ui-text-area`: multi-line plain text that an application edits.
//!
//! The application owns the whole text and sends only the part it shows:
//! whole lines from its first visible line, each cut to the columns it shows,
//! joined by `'\n'`. The node says where the caret and the selection anchor
//! fall in that part (`Node::scroll_y`, see [`pack`]) and where the shown part
//! sits in the whole text for the scroll bar (`Node::value`, 0..=10000, or
//! 10001 when everything fits). Paint, hit testing and the application's own
//! scrolling share the placement below, so a press lands on the character it
//! is drawn over.
use crate::layout::Rect;

const FIELD: u32 = 13;
const MASK: u32 = (1 << FIELD) - 1;
/// Set when the caret is outside the shown part (scrolled away): its field
/// then holds the nearer end of the shown part, so a selection that runs
/// past an edge is still drawn to that edge, and no caret is drawn.
const AWAY: u32 = 1 << (2 * FIELD);
/// The largest packed value.
pub const PACKED_MAX: u32 = (AWAY << 1) - 1;

/// Caret and selection anchor as byte offsets into the shown part. Without a
/// selection the anchor equals the caret. A caret or anchor outside the shown
/// part is clamped to its start or end by the application.
pub const fn pack(caret: usize, anchor: usize, caret_shown: bool) -> u32 {
    const fn field(at: usize) -> u32 {
        if at <= MASK as usize {
            at as u32
        } else {
            MASK
        }
    }
    field(caret) | field(anchor) << FIELD | if caret_shown { 0 } else { AWAY }
}
pub const fn caret(scroll_y: u32) -> usize {
    (scroll_y & MASK) as usize
}
pub const fn anchor(scroll_y: u32) -> usize {
    ((scroll_y >> FIELD) & MASK) as usize
}
/// Whether the caret is drawn: it is in the shown part.
pub const fn caret_shown(scroll_y: u32) -> bool {
    scroll_y & AWAY == 0
}
/// The selected bytes of the shown part, possibly empty.
pub fn selection(scroll_y: u32) -> core::ops::Range<usize> {
    let (c, a) = (caret(scroll_y), anchor(scroll_y));
    c.min(a)..c.max(a)
}

/// `input::EDIT` values: one key with its modifiers. The application decides
/// what each does to its text; the host only names the key.
pub mod edit {
    pub const LEFT: i32 = 1;
    pub const RIGHT: i32 = 2;
    pub const UP: i32 = 3;
    pub const DOWN: i32 = 4;
    pub const HOME: i32 = 5;
    pub const END: i32 = 6;
    pub const PAGE_UP: i32 = 7;
    pub const PAGE_DOWN: i32 = 8;
    pub const DELETE: i32 = 9;
    /// Enter: a line break, never the area's activation.
    pub const NEWLINE: i32 = 10;
    pub const TAB: i32 = 11;
    pub const SELECT_ALL: i32 = 12;
    const LAST: i32 = SELECT_ALL;
    pub const SHIFT: i32 = 0x100;
    pub const CONTROL: i32 = 0x200;
    pub const fn key(value: i32) -> i32 {
        value & 0xff
    }
    pub const fn valid(value: i32) -> bool {
        value & !(SHIFT | CONTROL | 0xff) == 0 && key(value) >= 1 && key(value) <= LAST
    }
}

/// Distance between baselines at `size`: a quarter more than the glyph box.
pub const fn line_height(size: u16) -> u32 {
    (size as u32 * 5).div_ceil(4)
}
/// Rows that fit wholly in `height`, and rows at least partly drawn.
pub const fn rows(height: u32, size: u16) -> (usize, usize) {
    let line = line_height(size);
    ((height / line) as usize, height.div_ceil(line) as usize)
}

/// Width of the scroll strip at the right edge of the content box, present
/// while `Node::value` holds a position.
pub const BAR: u32 = 10;
const THUMB: u32 = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// Where the lines are drawn.
    pub text: Rect,
    pub track: Option<Rect>,
}
pub fn geometry(content: Rect, value: u16) -> Geometry {
    if value > 10000 || content.width <= BAR * 2 || content.height == 0 {
        return Geometry {
            text: content,
            track: None,
        };
    }
    Geometry {
        text: Rect {
            width: content.width - BAR - 2,
            ..content
        },
        track: Some(Rect {
            x: content.right() - BAR as i32,
            width: BAR,
            ..content
        }),
    }
}
impl Geometry {
    pub fn thumb(&self, value: u16) -> Option<Rect> {
        let track = self.track?;
        let height = THUMB.min(track.height);
        let travel = track.height - height;
        Some(Rect {
            y: track.y + (u64::from(travel) * u64::from(value.min(10000)) / 10000) as i32,
            height,
            ..track
        })
    }
    /// The position (0..=10000) that puts the thumb's middle under `y`.
    pub fn position(&self, y: i32) -> Option<u16> {
        let track = self.track?;
        let height = THUMB.min(track.height);
        let travel = i64::from(track.height - height);
        if travel == 0 {
            return Some(0);
        }
        let at = (i64::from(y) - i64::from(track.y) - i64::from(height / 2)).clamp(0, travel);
        Some((at * 10000 / travel) as u16)
    }
}

/// A tab advances the pen to the next multiple of four digit widths (the
/// UI font is proportional), kept on whole pixels so that text after a tab
/// is placed exactly as a run starting there.
fn tab_stop(size: u16) -> u32 {
    let digit = crate::font::ui_text_glyph('0', size).map_or(64 * 8, |g| g.advance());
    (digit * 4).div_ceil(64).max(1) * 64
}
fn advance(c: char, size: u16, pen: u32, stop: u32) -> u32 {
    if c == '\t' {
        return (pen / stop + 1) * stop;
    }
    pen.saturating_add(crate::font::ui_text_glyph(c, size).map_or(0, |g| g.advance()))
}
/// Every character boundary of one line with its pen position in whole
/// pixels, from `(0, 0)` to `(line.len(), width)`.
pub fn boundaries(line: &str, size: u16) -> impl Iterator<Item = (usize, i32)> + '_ {
    let stop = tab_stop(size);
    let mut pen = 0u32;
    let mut chars = line.char_indices();
    let mut done = false;
    core::iter::from_fn(move || {
        if done {
            return None;
        }
        let x = ((pen + 32) >> 6) as i32;
        match chars.next() {
            Some((at, c)) => {
                pen = advance(c, size, pen, stop);
                Some((at, x))
            }
            None => {
                done = true;
                Some((line.len(), x))
            }
        }
    })
}
/// Pen position of byte `at` (a character boundary) in `line`.
pub fn x_of(line: &str, at: usize, size: u16) -> i32 {
    boundaries(line, size)
        .take_while(|(b, _)| *b <= at)
        .last()
        .map_or(0, |(_, x)| x)
}
pub fn width(line: &str, size: u16) -> u32 {
    boundaries(line, size).last().map_or(0, |(_, x)| x as u32)
}
/// The boundary nearest to `x` in `line`.
pub fn offset_in_line(line: &str, size: u16, x: i32) -> usize {
    let mut best = (0, i32::MAX);
    for (at, pen) in boundaries(line, size) {
        let distance = (pen - x).abs();
        if distance < best.1 {
            best = (at, distance);
        } else if pen > x {
            break;
        }
    }
    best.0
}
/// The byte offset in `text` (the shown part) nearest to a point relative
/// to the top left of the text box. Rows below the last line map onto it.
pub fn offset_at(text: &str, size: u16, x: i32, y: i32) -> usize {
    let row = (y.max(0) as u32 / line_height(size)) as usize;
    let mut start = 0;
    for (i, line) in text.split('\n').enumerate() {
        if i == row {
            return start + offset_in_line(line, size, x);
        }
        start += line.len() + 1;
    }
    let last = text.rfind('\n').map_or(0, |n| n + 1);
    last + offset_in_line(&text[last..], size, x)
}
/// Runs of one line without tabs, each with its pen position: what a painter
/// draws, glyph for glyph as `font::ui_text_run` places a run starting there.
pub fn runs(line: &str, size: u16) -> impl Iterator<Item = (i32, &str)> + '_ {
    let stop = tab_stop(size);
    let mut rest = line;
    let mut pen = 0u32;
    core::iter::from_fn(move || loop {
        if rest.is_empty() {
            return None;
        }
        if let Some(after) = rest.strip_prefix('\t') {
            pen = (pen / stop + 1) * stop;
            rest = after;
            continue;
        }
        let end = rest.find('\t').unwrap_or(rest.len());
        let (run, after) = rest.split_at(end);
        let x = ((pen + 32) >> 6) as i32;
        // A run starts on a whole pixel only after a tab; the first run starts
        // at zero. Either way the run's own rounding matches `boundaries`.
        for c in run.chars() {
            pen = advance(c, size, pen, stop);
        }
        rest = after;
        return Some((x, run));
    })
}

/// Lines of a whole text: `'\n'` separates them, so there is one more line
/// than line breaks. Applications keep other line endings as metadata.
pub fn line_count(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count() + 1
}
/// Byte offset where line `line` (from 0) starts, or the text's end.
pub fn line_start(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    text.bytes()
        .enumerate()
        .filter(|(_, b)| *b == b'\n')
        .nth(line - 1)
        .map_or(text.len(), |(i, _)| i + 1)
}
/// The line holding `at` and that line's start.
pub fn line_of(text: &str, at: usize) -> (usize, usize) {
    let at = at.min(text.len());
    let before = &text.as_bytes()[..at];
    let line = before.iter().filter(|b| **b == b'\n').count();
    let start = before
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |n| n + 1);
    (line, start)
}
fn line_end(text: &str, start: usize) -> usize {
    text[start..].find('\n').map_or(text.len(), |n| start + n)
}

/// Where an area's view of a whole text starts: its first line and the
/// characters cut from the start of every shown line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scroll {
    pub top: usize,
    pub left: usize,
}
/// Most rows a frame can show.
pub const MAX_ROWS: usize = 192;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Row {
    /// Whole-text offset of the first shown byte.
    source: usize,
    /// Its offset in the shown part, and the shown bytes of the line.
    shown: u16,
    len: u16,
}
/// One composed view: the map from the shown part back to the whole text.
/// Keep the frame of each submitted revision and read CARET and SELECT
/// offsets with the frame of the revision the event names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    rows: [Row; MAX_ROWS],
    len: usize,
    /// Rows that fit wholly, and the text's lines when composed.
    pub full_rows: usize,
    pub lines: usize,
}
impl Frame {
    pub const EMPTY: Self = Self {
        rows: [Row {
            source: 0,
            shown: 0,
            len: 0,
        }; MAX_ROWS],
        len: 0,
        full_rows: 0,
        lines: 1,
    };
    /// The whole-text offset of a shown offset (a CARET or SELECT value).
    pub fn source(&self, shown: usize) -> Option<usize> {
        let rows = &self.rows[..self.len];
        let row = rows.iter().rev().find(|r| usize::from(r.shown) <= shown)?;
        let within = (shown - usize::from(row.shown)).min(usize::from(row.len));
        Some(row.source + within)
    }
    /// The shown offset of a whole-text offset, clamped to the nearest shown
    /// byte, and whether it is in view.
    fn shown(&self, at: usize, text: &str) -> (usize, bool) {
        let rows = &self.rows[..self.len];
        let Some(first) = rows.first() else {
            return (0, false);
        };
        if at < first.source {
            return (0, false);
        }
        for (i, row) in rows.iter().enumerate() {
            let end = row.source + usize::from(row.len);
            let next = rows.get(i + 1).map_or(usize::MAX, |r| r.source);
            if at < row.source {
                // In the cut-off start of this row's line.
                return (usize::from(row.shown), false);
            }
            if at <= end {
                return (usize::from(row.shown) + at - row.source, true);
            }
            let line_end = line_end(text, row.source);
            if at <= line_end || at < next {
                // Past the shown columns of this line.
                return (usize::from(row.shown + row.len), false);
            }
        }
        let last = rows[rows.len() - 1];
        (usize::from(last.shown + last.len), false)
    }
    /// The first line of the text a position strip value puts at the top.
    pub fn top_for(&self, value: u16) -> usize {
        let range = self.lines.saturating_sub(self.full_rows.max(1));
        range * usize::from(value.min(10000)) / 10000
    }
}
/// What an application puts in its `ui-text-area` node for one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Composed {
    pub len: usize,
    /// `Node::scroll_y`.
    pub packed: u32,
    /// `Node::value`.
    pub value: u16,
}
/// Compose the shown part of `text` for a text box of `width` x `height`
/// pixels at `size`, into `out` (at most `MAX_AREA_TEXT` bytes are used).
/// With `follow` the scroll first moves so that the caret is in view.
/// `caret` and `anchor` are whole-text offsets on character boundaries.
#[allow(clippy::too_many_arguments)]
pub fn compose(
    text: &str,
    scroll: &mut Scroll,
    caret: usize,
    anchor: usize,
    follow: bool,
    size: u16,
    width: u32,
    height: u32,
    out: &mut [u8],
    frame: &mut Frame,
) -> Composed {
    let out_len = out.len().min(crate::MAX_AREA_TEXT);
    let out = &mut out[..out_len];
    let (full, drawn) = rows(height, size);
    let full = full.max(1);
    let drawn = drawn.clamp(1, MAX_ROWS);
    let lines = line_count(text);
    if follow {
        let (line, start) = line_of(text, caret);
        if line < scroll.top {
            scroll.top = line;
        } else if line >= scroll.top + full {
            scroll.top = line + 1 - full;
        }
        // Columns: keep the caret off the right edge by a few characters.
        let column = text[start..caret.min(text.len())].chars().count();
        let line_text = &text[start..line_end(text, start)];
        if column < scroll.left {
            scroll.left = column.saturating_sub(4);
        }
        loop {
            let cut = line_text
                .char_indices()
                .nth(scroll.left)
                .map_or(line_text.len(), |(i, _)| i);
            let reach = caret - start;
            let x = if reach >= cut {
                x_of(&line_text[cut..], reach - cut, size)
            } else {
                0
            };
            let margin = (u32::from(size) * 2) as i32;
            if x <= width as i32 - margin || reach <= cut {
                break;
            }
            scroll.left += 8;
        }
    }
    scroll.top = scroll.top.min(lines.saturating_sub(1));
    let mut used = 0;
    frame.len = 0;
    frame.full_rows = full;
    frame.lines = lines;
    let mut start = line_start(text, scroll.top);
    for _ in 0..drawn {
        if frame.len != 0 {
            if start > text.len() || used >= out.len() {
                break;
            }
            out[used] = b'\n';
            used += 1;
        }
        let end = line_end(text, start);
        let line = &text[start..end];
        let cut = line
            .char_indices()
            .nth(scroll.left)
            .map_or(line.len(), |(i, _)| i);
        // Characters up to just past the right edge, within the budget.
        let mut shown = 0;
        for (at, x) in boundaries(&line[cut..], size) {
            if at > out.len() - used {
                break;
            }
            shown = at;
            if x > width as i32 {
                break;
            }
        }
        out[used..used + shown].copy_from_slice(line[cut..cut + shown].as_bytes());
        frame.rows[frame.len] = Row {
            source: start + cut,
            shown: used as u16,
            len: shown as u16,
        };
        frame.len += 1;
        used += shown;
        if end >= text.len() {
            break;
        }
        start = end + 1;
    }
    let shown = core::str::from_utf8(&out[..used]).unwrap_or("");
    let (c, in_view) = frame.shown(caret, text);
    let (a, _) = frame.shown(anchor, text);
    let range = lines.saturating_sub(full);
    Composed {
        len: shown.len(),
        packed: pack(c, a, in_view),
        value: if range == 0 {
            10001
        } else {
            (scroll.top.min(range) * 10000 / range) as u16
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Space,
    Word,
    Mark,
}
fn class(c: char) -> Class {
    if c.is_whitespace() {
        Class::Space
    } else if c.is_alphanumeric() || c == '_' {
        Class::Word
    } else {
        Class::Mark
    }
}
/// Start of the word before `at` (Ctrl+Left).
pub fn word_before(text: &str, at: usize) -> usize {
    let mut chars = text[..at].char_indices().rev().peekable();
    while chars
        .next_if(|(_, c)| class(*c) == Class::Space && *c != '\n')
        .is_some()
    {}
    let Some(&(i, c)) = chars.peek() else {
        return 0;
    };
    if c == '\n' {
        return if i + 1 == at { i } else { i + 1 };
    }
    let kind = class(c);
    let mut start = i;
    for (j, d) in chars {
        if class(d) != kind || d == '\n' {
            break;
        }
        start = j;
    }
    start
}
/// Start of the word after `at` (Ctrl+Right).
pub fn word_after(text: &str, at: usize) -> usize {
    let rest = &text[at..];
    let mut chars = rest.char_indices().peekable();
    let Some(&(_, c)) = chars.peek() else {
        return at;
    };
    if c == '\n' {
        return at + 1;
    }
    let kind = class(c);
    while chars
        .next_if(|(_, d)| class(*d) == kind && *d != '\n')
        .is_some()
    {}
    while chars
        .next_if(|(_, d)| class(*d) == Class::Space && *d != '\n')
        .is_some()
    {}
    chars.peek().map_or(text.len(), |(i, _)| at + i)
}
/// Where an editing key moves the caret. `column` keeps the pixel column
/// that Up and Down return to; other keys clear it. `page` is the rows of a
/// page. Delete, Tab, Enter and select-all are the application's to apply.
pub fn motion(
    text: &str,
    caret: usize,
    value: i32,
    size: u16,
    page: usize,
    column: &mut Option<i32>,
) -> usize {
    let control = value & edit::CONTROL != 0;
    let caret = caret.min(text.len());
    let (line, start) = line_of(text, caret);
    let vertical = |lines: isize, column: &mut Option<i32>| {
        let x = *column
            .get_or_insert_with(|| x_of(&text[start..line_end(text, start)], caret - start, size));
        let target = (line as isize + lines).clamp(0, line_count(text) as isize - 1) as usize;
        let to = line_start(text, target);
        to + offset_in_line(&text[to..line_end(text, to)], size, x)
    };
    let key = edit::key(value);
    if !matches!(key, edit::UP | edit::DOWN | edit::PAGE_UP | edit::PAGE_DOWN) {
        *column = None;
    }
    match key {
        edit::LEFT if control => word_before(text, caret),
        edit::LEFT => text[..caret]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i),
        edit::RIGHT if control => word_after(text, caret),
        edit::RIGHT => text[caret..]
            .chars()
            .next()
            .map_or(caret, |c| caret + c.len_utf8()),
        edit::UP => vertical(-1, column),
        edit::DOWN => vertical(1, column),
        edit::PAGE_UP => vertical(-(page.max(1) as isize), column),
        edit::PAGE_DOWN => vertical(page.max(1) as isize, column),
        edit::HOME if control => 0,
        edit::HOME => start,
        edit::END if control => text.len(),
        edit::END => line_end(text, start),
        _ => caret,
    }
}

/// Validation of a node's packed fields against its shown text.
pub(crate) fn valid(text: &str, scroll_y: u32, value: u16) -> bool {
    let fits = |at: usize| at <= text.len() && text.is_char_boundary(at);
    scroll_y <= PACKED_MAX && fits(caret(scroll_y)) && fits(anchor(scroll_y)) && value <= 10001
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packing_round_trips_and_marks_a_caret_out_of_view() {
        let v = pack(17, 3, true);
        assert_eq!((caret(v), anchor(v), caret_shown(v)), (17, 3, true));
        assert_eq!(selection(v), 3..17);
        let v = pack(0, 9, false);
        assert_eq!((caret(v), anchor(v), caret_shown(v)), (0, 9, false));
        assert!(pack(6144, 6144, false) <= PACKED_MAX);
    }
    #[test]
    fn boundaries_include_both_ends_and_tabs_reach_a_stop() {
        let b: [(usize, i32); 3] = {
            let mut it = boundaries("a\tb", 16);
            [it.next().unwrap(), it.nth(1).unwrap(), it.next().unwrap()]
        };
        assert_eq!(b[0], (0, 0));
        assert_eq!(b[1].0, 2);
        let stop = (tab_stop(16) >> 6) as i32;
        assert_eq!(b[1].1, stop);
        assert!(b[2].1 > stop);
        // A run after the tab is drawn where the boundary puts it.
        let runs: [(i32, &str); 2] = {
            let mut it = runs("a\tb", 16);
            [it.next().unwrap(), it.next().unwrap()]
        };
        assert_eq!(runs, [(0, "a"), (stop, "b")]);
    }
    #[test]
    fn hit_testing_finds_the_nearest_boundary_on_the_row() {
        let text = "ab\ncd";
        let h = line_height(16) as i32;
        assert_eq!(offset_at(text, 16, -5, 0), 0);
        assert_eq!(offset_at(text, 16, 500, 0), 2);
        assert_eq!(offset_at(text, 16, 0, h + 1), 3);
        assert_eq!(offset_at(text, 16, 500, h * 9), 5);
        let x = x_of("cd", 1, 16);
        assert_eq!(offset_at(text, 16, x, h), 4);
    }
    #[test]
    fn the_thumb_follows_the_position() {
        let g = geometry(
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 132,
            },
            0,
        );
        let track = g.track.unwrap();
        assert_eq!(g.text.width, 200 - BAR - 2);
        assert_eq!(g.thumb(0).unwrap().y, 0);
        assert_eq!(g.thumb(10000).unwrap().bottom(), track.bottom());
        assert_eq!(g.position(0), Some(0));
        assert_eq!(g.position(1000), Some(10000));
        assert_eq!(g.position(66), Some(5000));
        assert!(geometry(g.text, 10001).track.is_none());
    }
    #[test]
    fn edit_values_name_one_key_and_known_modifiers() {
        assert!(edit::valid(edit::LEFT | edit::SHIFT | edit::CONTROL));
        assert!(!edit::valid(0));
        assert!(!edit::valid(13));
        assert!(!edit::valid(edit::LEFT | 0x800));
    }

    #[test]
    fn lines_are_found_by_breaks() {
        let t = "ab\ncd\n";
        assert_eq!(line_count(t), 3);
        assert_eq!(line_start(t, 1), 3);
        assert_eq!(line_start(t, 2), 6);
        assert_eq!(line_start(t, 9), 6);
        assert_eq!(line_of(t, 4), (1, 3));
        assert_eq!(line_of(t, 6), (2, 6));
    }
    #[test]
    fn composing_shows_lines_from_the_top_and_maps_offsets_back() {
        let text = "one\ntwo\nthree\nfour\nfive";
        let mut scroll = Scroll { top: 1, left: 0 };
        let mut out = [0u8; 256];
        let mut frame = Frame::EMPTY;
        let h = line_height(16) * 2;
        let c = compose(
            text,
            &mut scroll,
            9,
            9,
            false,
            16,
            400,
            h,
            &mut out,
            &mut frame,
        );
        let shown = core::str::from_utf8(&out[..c.len]).unwrap();
        assert_eq!(shown, "two\nthree");
        assert_eq!((caret(c.packed), caret_shown(c.packed)), (5, true));
        assert_eq!(frame.source(5), Some(9));
        assert_eq!(frame.source(0), Some(4));
        // Four lines can be the top of five shown two at a time.
        assert_eq!(c.value, 10000 / 3);
        assert_eq!(frame.top_for(10000), 3);
        // A caret below the view is clamped to the end and hidden...
        let c = compose(
            text,
            &mut scroll,
            20,
            0,
            false,
            16,
            400,
            h,
            &mut out,
            &mut frame,
        );
        assert_eq!((caret(c.packed), caret_shown(c.packed)), (9, false));
        assert_eq!(anchor(c.packed), 0);
        // ...and following it scrolls down to it.
        let c = compose(
            text,
            &mut scroll,
            20,
            20,
            true,
            16,
            400,
            h,
            &mut out,
            &mut frame,
        );
        assert_eq!(scroll.top, 3);
        let shown = core::str::from_utf8(&out[..c.len]).unwrap();
        assert_eq!(shown, "four\nfive");
        assert_eq!(caret(c.packed), 6);
    }
    #[test]
    fn long_lines_scroll_sideways_to_the_caret() {
        let line = "x".repeat(300);
        let mut scroll = Scroll::default();
        let mut out = [0u8; 1024];
        let mut frame = Frame::EMPTY;
        let c = compose(
            &line,
            &mut scroll,
            300,
            300,
            true,
            16,
            200,
            40,
            &mut out,
            &mut frame,
        );
        assert!(scroll.left > 0);
        assert!(caret_shown(c.packed));
        assert_eq!(frame.source(caret(c.packed)), Some(300));
        assert!(c.len < 300);
    }
    #[test]
    fn motion_follows_words_lines_and_columns() {
        let t = "hello world\nab";
        let mut column = None;
        assert_eq!(
            motion(t, 0, edit::RIGHT | edit::CONTROL, 16, 10, &mut column),
            6
        );
        assert_eq!(
            motion(t, 11, edit::LEFT | edit::CONTROL, 16, 10, &mut column),
            6
        );
        assert_eq!(
            motion(t, 6, edit::LEFT | edit::CONTROL, 16, 10, &mut column),
            0
        );
        assert_eq!(motion(t, 3, edit::END, 16, 10, &mut column), 11);
        assert_eq!(
            motion(t, 3, edit::END | edit::CONTROL, 16, 10, &mut column),
            t.len()
        );
        let down = motion(t, 10, edit::DOWN, 16, 10, &mut column);
        assert_eq!(down, t.len());
        // Up returns to the remembered column, not the short line's end.
        assert_eq!(motion(t, down, edit::UP, 16, 10, &mut column), 10);
        assert_eq!(motion(t, 13, edit::HOME, 16, 10, &mut column), 12);
    }
}
