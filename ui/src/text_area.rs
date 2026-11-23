//! `ui-text-area`: multi-line plain text that an application edits.
//!
//! The application owns the whole text and sends only the part it shows:
//! whole lines from its first visible line, each cut to the columns it shows,
//! joined by `'\n'`. The node says where the caret and the selection anchor
//! fall in that part (`Node::scroll_y`, see [`pack`]) and where the shown part
//! sits in the whole text for the scroll bar (`Node::value`, 0..=10000, or
//! 10001 when everything fits). With word wrap a long line is shown as
//! several rows, each its own `'\n'`-separated part. Without it, lines wider
//! than the box get a horizontal scrollbar along the bottom (`Node::scroll_x`, see
//! [`encode_horizontal_scroll`]). Paint, hit testing and the application's own scrolling
//! share the placement below, so a press lands on the character it is drawn
//! over.
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
/// while `Node::value` holds a position, and height of the horizontal scrollbar
/// along the bottom while `Node::scroll_x` holds one.
pub const BAR: u32 = 10;
const THUMB: u32 = 32;
const HORIZONTAL_THUMB: u32 = 48;
/// A VALUE event at `HORIZONTAL_SCROLL_EVENT_BASE + p` moves the horizontal scrollbar to `p`
/// (0..=10000); a plain VALUE moves the strip at the right edge.
pub const HORIZONTAL_SCROLL_EVENT_BASE: i32 = 0x1_0000;

/// Extract the position from a horizontal scrollbar VALUE event.
pub fn horizontal_scroll_event_position(value: i32) -> Option<u16> {
    let position = value.checked_sub(HORIZONTAL_SCROLL_EVENT_BASE)?;
    (0..=10_000).contains(&position).then_some(position as u16)
}

/// `Node::scroll_x` for a horizontal scrollbar at `position` (0..=10000), or none.
/// The node keeps the position in 255 steps, enough to place its thumb.
pub const fn encode_horizontal_scroll(position: Option<u16>) -> u8 {
    match position {
        Some(p) => {
            let p = if p > 10000 { 10000 } else { p };
            1 + ((p as u32 * 254 + 5000) / 10000) as u8
        }
        None => 0,
    }
}
/// The horizontal scrollbar's position (0..=10000) in `Node::scroll_x`, if any.
pub const fn decode_horizontal_scroll(scroll_x: u8) -> Option<u16> {
    if scroll_x == 0 {
        None
    } else {
        Some(((scroll_x as u32 - 1) * 10000 / 254) as u16)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    /// Where the lines are drawn.
    pub text: Rect,
    pub track: Option<Rect>,
    /// The horizontal scrollbar along the bottom.
    pub horizontal_track: Option<Rect>,
}
pub fn geometry(content: Rect, value: u16) -> Geometry {
    geometry_with_scroll(content, value, 0)
}
/// The placement of an area with strip position `value` (`Node::value`)
/// and horizontal scrollbar `scroll_x` (`Node::scroll_x`).
pub fn geometry_with_scroll(content: Rect, value: u16, scroll_x: u8) -> Geometry {
    let mut g = Geometry {
        text: content,
        track: None,
        horizontal_track: None,
    };
    if content.width <= BAR * 2 || content.height == 0 {
        return g;
    }
    if decode_horizontal_scroll(scroll_x).is_some() && content.height > BAR * 3 {
        g.text.height = content.height - BAR - 2;
        g.horizontal_track = Some(Rect {
            y: content.bottom() - BAR as i32,
            height: BAR,
            ..content
        });
    }
    if value <= 10000 {
        g.text.width = content.width - BAR - 2;
        g.track = Some(Rect {
            x: content.right() - BAR as i32,
            width: BAR,
            height: g.text.height,
            ..content
        });
        if let Some(side) = g.horizontal_track.as_mut() {
            side.width = g.text.width;
        }
    }
    g
}
impl Geometry {
    pub fn horizontal_thumb(&self, scroll_x: u8) -> Option<Rect> {
        let side = self.horizontal_track?;
        let width = HORIZONTAL_THUMB.min(side.width);
        let travel = side.width - width;
        let at = decode_horizontal_scroll(scroll_x).unwrap_or(0);
        Some(Rect {
            x: side.x + (u64::from(travel) * u64::from(at.min(10000)) / 10000) as i32,
            width,
            ..side
        })
    }
    /// The horizontal position (0..=10000) that puts its thumb's middle at `x`.
    pub fn horizontal_position(&self, x: i32) -> Option<u16> {
        let side = self.horizontal_track?;
        let width = HORIZONTAL_THUMB.min(side.width);
        let travel = i64::from(side.width - width);
        if travel == 0 {
            return Some(0);
        }
        let at = (i64::from(x) - i64::from(side.x) - i64::from(width / 2)).clamp(0, travel);
        Some((at * 10000 / travel) as u16)
    }
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
    /// The row begins its line (it is not the continuation of a wrapped one).
    start: bool,
    /// When the line continues on the next row: the bytes of the row's last
    /// character. The offset where the next row starts belongs to that row.
    tail: u8,
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
    /// The line of the first shown row, and the first lines the position
    /// strip can put at the top (its 0..=10000 range).
    pub first_line: usize,
    pub range: usize,
}
impl Frame {
    pub const EMPTY: Self = Self {
        rows: [Row {
            source: 0,
            shown: 0,
            len: 0,
            start: true,
            tail: 0,
        }; MAX_ROWS],
        len: 0,
        full_rows: 0,
        lines: 1,
        first_line: 0,
        range: 0,
    };
    /// The whole-text offset of a shown offset (a CARET or SELECT value).
    pub fn source(&self, shown: usize) -> Option<usize> {
        let rows = &self.rows[..self.len];
        let row = rows.iter().rev().find(|r| usize::from(r.shown) <= shown)?;
        let within = (shown - usize::from(row.shown)).min(usize::from(row.len));
        if within == usize::from(row.len) && row.tail != 0 {
            // Past the end of a wrapped row: before its last character, so
            // the caret stays on this row.
            return Some(row.source + within - usize::from(row.tail));
        }
        Some(row.source + within)
    }
    /// The shown rows' line numbers, one per row joined by `'\n'`, blank on
    /// the continuation rows of a wrapped line. Returns the bytes written.
    pub fn numbers(&self, out: &mut [u8]) -> usize {
        let mut used = 0;
        let mut line = self.first_line;
        for (i, row) in self.rows[..self.len].iter().enumerate() {
            if i != 0 {
                if row.start {
                    line += 1;
                }
                if used >= out.len() {
                    break;
                }
                out[used] = b'\n';
                used += 1;
            }
            if row.start {
                let mut digits = [0u8; 20];
                let mut n = line + 1;
                let mut k = digits.len();
                loop {
                    k -= 1;
                    digits[k] = b'0' + (n % 10) as u8;
                    n /= 10;
                    if n == 0 {
                        break;
                    }
                }
                let number = &digits[k..];
                if used + number.len() > out.len() {
                    break;
                }
                out[used..used + number.len()].copy_from_slice(number);
                used += number.len();
            }
        }
        used
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
        let _ = text;
        for (i, row) in rows.iter().enumerate() {
            let end = row.source + usize::from(row.len);
            let next = rows.get(i + 1).map_or(usize::MAX, |r| r.source);
            if at < row.source {
                // In the cut-off start of this row's line.
                return (usize::from(row.shown), false);
            }
            if at < end || (at == end && row.tail == 0) {
                return (usize::from(row.shown) + at - row.source, true);
            }
            if at < next {
                // Past the shown columns of this line.
                return (usize::from(row.shown + row.len), false);
            }
        }
        let last = rows[rows.len() - 1];
        (usize::from(last.shown + last.len), false)
    }
    /// The first line of the text a position strip value puts at the top.
    pub fn top_for(&self, value: u16) -> usize {
        self.range * usize::from(value.min(10000)) / 10000
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
    /// `Node::scroll_x`: the horizontal scrollbar, see [`encode_horizontal_scroll`].
    pub scroll_x: u8,
}
/// How lines wider than the box are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Break them into rows. `Scroll::left` then counts the rows of the top
    /// line scrolled away, not columns.
    pub wrap: bool,
    /// Without wrap: the characters of the text's longest line, for the
    /// horizontal scrollbar (zero for none).
    pub longest: usize,
}
/// Columns of digits that fit in `width`: the unit of the horizontal scrollbar.
fn columns(size: u16, width: u32) -> usize {
    let digit = crate::font::ui_text_glyph('0', size).map_or(64 * 8, |g| g.advance().max(1));
    (u64::from(width) * 64 / u64::from(digit)) as usize
}
/// Columns the horizontal scrollbar moves across; zero when the longest line fits.
fn horizontal_scroll_range(longest: usize, size: u16, width: u32) -> usize {
    (longest + 4).saturating_sub(columns(size, width))
}
/// Whether an unwrapped area of text box `width` needs the horizontal scrollbar.
/// Decide this before [`geometry_with_scroll`]: the strip takes the bottom rows.
pub fn needs_horizontal_scrollbar(options: Options, scroll: Scroll, size: u16, width: u32) -> bool {
    !options.wrap
        && (horizontal_scroll_range(options.longest, size, width) != 0 || scroll.left != 0)
}
/// The first shown column the horizontal scrollbar puts at `position`.
pub fn left_for(longest: usize, size: u16, width: u32, position: u16) -> usize {
    horizontal_scroll_range(longest, size, width) * usize::from(position.min(10000)) / 10000
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
    compose_with(
        text,
        scroll,
        caret,
        anchor,
        follow,
        size,
        width,
        height,
        out,
        frame,
        Options::default(),
    )
}
/// [`compose`], wrapping lines or measuring the horizontal scrollbar by `options`.
#[allow(clippy::too_many_arguments)]
pub fn compose_with(
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
    options: Options,
) -> Composed {
    let out_len = out.len().min(crate::MAX_AREA_TEXT);
    let out = &mut out[..out_len];
    let (full, drawn) = rows(height, size);
    let full = full.max(1);
    let drawn = drawn.clamp(1, MAX_ROWS);
    let lines = line_count(text);
    frame.len = 0;
    frame.full_rows = full;
    frame.lines = lines;
    let (used, complete) = if options.wrap {
        compose_wrapped(
            text, scroll, caret, follow, size, width, full, drawn, out, frame,
        )
    } else {
        compose_cut(
            text, scroll, caret, follow, size, width, full, drawn, out, frame,
        )
    };
    let shown = core::str::from_utf8(&out[..used]).unwrap_or("");
    let (c, in_view) = frame.shown(caret, text);
    let (a, _) = frame.shown(anchor, text);
    // Lines that can be the top. A few long wrapped lines still scroll.
    let mut range = lines.saturating_sub(full);
    if range == 0 && options.wrap && (!complete || scroll.left != 0) {
        range = lines.saturating_sub(1).max(1);
    }
    frame.range = range;
    let side = horizontal_scroll_range(options.longest, size, width);
    let scroll_x = if options.wrap || (side == 0 && scroll.left == 0) {
        0
    } else if side == 0 {
        // Scrolled to a caret past an estimate that said the lines fit.
        self::encode_horizontal_scroll(Some(10000))
    } else {
        self::encode_horizontal_scroll(Some((scroll.left.min(side) * 10000 / side) as u16))
    };
    Composed {
        len: shown.len(),
        packed: pack(c, a, in_view),
        value: if range == 0 {
            10001
        } else {
            (scroll.top.min(range) * 10000 / range) as u16
        },
        scroll_x,
    }
}
/// Whole lines from `scroll.top`, each cut by `scroll.left` columns. Returns
/// the bytes used and whether the text's end is shown.
#[allow(clippy::too_many_arguments)]
fn compose_cut(
    text: &str,
    scroll: &mut Scroll,
    caret: usize,
    follow: bool,
    size: u16,
    width: u32,
    full: usize,
    drawn: usize,
    out: &mut [u8],
    frame: &mut Frame,
) -> (usize, bool) {
    let lines = frame.lines;
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
    frame.first_line = scroll.top;
    let mut used = 0;
    let mut start = line_start(text, scroll.top);
    let mut complete = false;
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
            start: true,
            tail: 0,
        };
        frame.len += 1;
        used += shown;
        if end >= text.len() {
            complete = true;
            break;
        }
        start = end + 1;
    }
    (used, complete)
}
/// Where each row of `line` ends when it wraps at `width` pixels: after the
/// last space that fits (spaces hang past the edge), or inside a word wider
/// than a row. The last end is `line.len()`; an empty line is one row.
pub fn wrap_ends(line: &str, size: u16, width: u32) -> impl Iterator<Item = usize> + '_ {
    let mut start = 0;
    let mut done = false;
    core::iter::from_fn(move || {
        if done {
            return None;
        }
        let rest = &line[start..];
        let mut fit = 0;
        let mut space = 0;
        let mut previous = None;
        for (at, x) in boundaries(rest, size) {
            if at > 0 && x > width as i32 {
                break;
            }
            fit = at;
            if at > 0 && matches!(previous, Some(' ' | '\t')) {
                space = at;
            }
            previous = rest[at..].chars().next();
        }
        if fit == rest.len() {
            done = true;
            return Some(line.len());
        }
        let mut end = if space > 0 {
            space
        } else if fit > 0 {
            fit
        } else {
            rest.chars().next().map_or(rest.len(), char::len_utf8)
        };
        while rest[end..].starts_with(' ') {
            end += 1;
        }
        if end >= rest.len() {
            done = true;
            return Some(line.len());
        }
        start += end;
        Some(start)
    })
}
/// The rows of `line`, as `(start, end)` offsets in it.
fn wrapped_rows(line: &str, size: u16, width: u32) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut start = 0;
    wrap_ends(line, size, width).map(move |end| {
        let row = (start, end);
        start = end;
        row
    })
}
/// The row of `line` holding byte `at` and its bounds: the offset where a
/// row ends belongs to the next row, the line's end to its last row.
fn row_at(line: &str, at: usize, size: u16, width: u32) -> (usize, usize, usize) {
    let mut found = (0, 0, line.len());
    for (i, (start, end)) in wrapped_rows(line, size, width).enumerate() {
        found = (i, start, end);
        if at < end {
            break;
        }
    }
    found
}
fn row_count(line: &str, size: u16, width: u32) -> usize {
    wrap_ends(line, size, width).count()
}
/// Rows from row `scroll.left` of line `scroll.top`, every line broken to
/// fit `width`. Returns the bytes used and whether the text's end is shown.
#[allow(clippy::too_many_arguments)]
fn compose_wrapped(
    text: &str,
    scroll: &mut Scroll,
    caret: usize,
    follow: bool,
    size: u16,
    width: u32,
    full: usize,
    drawn: usize,
    out: &mut [u8],
    frame: &mut Frame,
) -> (usize, bool) {
    let lines = frame.lines;
    let line_at = |line: usize| {
        let start = line_start(text, line);
        (start, &text[start..line_end(text, start)])
    };
    scroll.top = scroll.top.min(lines.saturating_sub(1));
    scroll.left = scroll
        .left
        .min(row_count(line_at(scroll.top).1, size, width) - 1);
    if follow {
        let (line, start) = line_of(text, caret);
        let (row, _, _) = row_at(line_at(line).1, caret - start, size, width);
        if (line, row) < (scroll.top, scroll.left) {
            scroll.top = line;
            scroll.left = row;
        } else {
            // Rows from the top through the caret's row, until a screen.
            let mut count = 0;
            let mut l = scroll.top;
            while l <= line && count <= full {
                let rows = if l == line {
                    row + 1
                } else {
                    row_count(line_at(l).1, size, width)
                };
                count += rows - if l == scroll.top { scroll.left } else { 0 };
                l += 1;
            }
            if count > full {
                // The caret's row becomes the last whole row.
                let (mut l, mut r) = (line, row);
                let mut need = full - 1;
                while need > 0 {
                    if r >= need {
                        r -= need;
                        need = 0;
                    } else if l == 0 {
                        r = 0;
                        need = 0;
                    } else {
                        need -= r + 1;
                        l -= 1;
                        r = row_count(line_at(l).1, size, width) - 1;
                    }
                }
                scroll.top = l;
                scroll.left = r;
            }
        }
    }
    frame.first_line = scroll.top;
    let mut used = 0;
    let mut line = scroll.top;
    let mut skip = scroll.left;
    let mut complete = false;
    'lines: while frame.len < drawn {
        let (start, text_line) = line_at(line);
        for (i, (from, to)) in wrapped_rows(text_line, size, width).enumerate() {
            if i < skip {
                continue;
            }
            if frame.len == drawn {
                break 'lines;
            }
            if frame.len != 0 {
                if used >= out.len() {
                    break 'lines;
                }
                out[used] = b'\n';
                used += 1;
            }
            let row = &text_line[from..to];
            let mut shown = row.len().min(out.len() - used);
            while !row.is_char_boundary(shown) {
                shown -= 1;
            }
            out[used..used + shown].copy_from_slice(&row.as_bytes()[..shown]);
            let tail = if to < text_line.len() && shown == row.len() {
                row.chars().next_back().map_or(0, char::len_utf8) as u8
            } else {
                0
            };
            frame.rows[frame.len] = Row {
                source: start + from,
                shown: used as u16,
                len: shown as u16,
                start: i == 0,
                tail,
            };
            frame.len += 1;
            used += shown;
            if shown < row.len() {
                break 'lines;
            }
        }
        skip = 0;
        if line + 1 >= lines {
            complete = true;
            break;
        }
        line += 1;
    }
    (used, complete)
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

/// [`motion`] in an area that wraps lines at `width` pixels: Up, Down and
/// the page keys move by rows, Home and End to the ends of the caret's row.
#[allow(clippy::too_many_arguments)]
pub fn motion_wrapped(
    text: &str,
    caret: usize,
    value: i32,
    size: u16,
    page: usize,
    column: &mut Option<i32>,
    width: u32,
) -> usize {
    let key = edit::key(value);
    let control = value & edit::CONTROL != 0;
    let rowwise = matches!(key, edit::UP | edit::DOWN | edit::PAGE_UP | edit::PAGE_DOWN)
        || (matches!(key, edit::HOME | edit::END) && !control);
    if !rowwise {
        return motion(text, caret, value, size, page, column);
    }
    let caret = caret.min(text.len());
    let (mut line, start) = line_of(text, caret);
    let line_text = |line: usize| {
        let start = line_start(text, line);
        (start, &text[start..line_end(text, start)])
    };
    let (mut row, from, to) = row_at(
        &text[start..line_end(text, start)],
        caret - start,
        size,
        width,
    );
    match key {
        edit::HOME => {
            *column = None;
            return start + from;
        }
        edit::END => {
            *column = None;
            let line_len = line_end(text, start) - start;
            if to == line_len {
                return start + to;
            }
            // Before the row's last character: the end offset is the next
            // row's start.
            return start
                + text[start + from..start + to]
                    .char_indices()
                    .next_back()
                    .map_or(from, |(i, _)| from + i);
        }
        _ => {}
    }
    let x = *column
        .get_or_insert_with(|| x_of(&text[start + from..start + to], caret - start - from, size));
    let steps = match key {
        edit::UP | edit::DOWN => 1,
        _ => page.max(1),
    };
    let up = matches!(key, edit::UP | edit::PAGE_UP);
    let lines = line_count(text);
    for _ in 0..steps {
        if up {
            if row > 0 {
                row -= 1;
            } else if line > 0 {
                line -= 1;
                row = row_count(line_text(line).1, size, width) - 1;
            } else {
                break;
            }
        } else if row + 1 < row_count(line_text(line).1, size, width) {
            row += 1;
        } else if line + 1 < lines {
            line += 1;
            row = 0;
        } else {
            break;
        }
    }
    let (start, target) = line_text(line);
    let (from, to) = wrapped_rows(target, size, width)
        .nth(row)
        .unwrap_or((0, target.len()));
    let mut at = from + offset_in_line(&target[from..to], size, x);
    if at == to && to < target.len() {
        // The row's end is the next row's start: stay on this row.
        at = target[from..to]
            .char_indices()
            .next_back()
            .map_or(from, |(i, _)| from + i);
    }
    start + at
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
    fn wrapping_breaks_after_spaces_and_inside_long_words() {
        let w = width("aaaa ", 16);
        let ends: [usize; 3] = {
            let mut it = wrap_ends("aaaa bbbb cccc", 16, w + 2);
            [it.next().unwrap(), it.next().unwrap(), it.next().unwrap()]
        };
        assert_eq!(ends, [5, 10, 14]);
        assert_eq!(wrap_ends("", 16, 50).count(), 1);
        // A word wider than the row breaks inside it, never into an empty row.
        let long = "x".repeat(40);
        let rows = wrap_ends(&long, 16, width("xxxxx", 16)).count();
        assert!(rows >= 8);
        assert_eq!(wrap_ends(&long, 16, 0).count(), 40);
    }
    #[test]
    fn wrapped_rows_map_back_and_number_their_lines() {
        let text = "aaaa bbbb cccc\nshort\nlast";
        let w = width("aaaa ", 16) + 2;
        let mut scroll = Scroll::default();
        let mut out = [0u8; 256];
        let mut frame = Frame::EMPTY;
        let wrap = Options {
            wrap: true,
            longest: 0,
        };
        let h = line_height(16) * 10;
        let c = compose_with(
            text,
            &mut scroll,
            11,
            11,
            false,
            16,
            w,
            h,
            &mut out,
            &mut frame,
            wrap,
        );
        let shown = core::str::from_utf8(&out[..c.len]).unwrap();
        assert_eq!(shown, "aaaa \nbbbb \ncccc\nshort\nlast");
        assert_eq!(c.scroll_x, 0);
        assert_eq!(c.value, 10001);
        // Offset 11 is in "cccc", the third row.
        assert_eq!(caret(c.packed), 13);
        assert_eq!(frame.source(13), Some(11));
        // A press past a wrapped row's end stays on that row.
        assert_eq!(frame.source(5), Some(4));
        // The row boundary itself belongs to the next row.
        let c = compose_with(
            text,
            &mut scroll,
            5,
            5,
            false,
            16,
            w,
            h,
            &mut out,
            &mut frame,
            wrap,
        );
        assert_eq!(caret(c.packed), 6);
        let mut numbers = [0u8; 64];
        let n = frame.numbers(&mut numbers);
        assert_eq!(&numbers[..n], b"1\n\n\n2\n3");
        // Following a caret below scrolls by rows, into the middle of a line.
        let small = line_height(16) * 2;
        let c = compose_with(
            text,
            &mut scroll,
            11,
            11,
            true,
            16,
            w,
            small,
            &mut out,
            &mut frame,
            wrap,
        );
        assert_eq!((scroll.top, scroll.left), (0, 1));
        let shown = core::str::from_utf8(&out[..c.len]).unwrap();
        assert_eq!(shown, "bbbb \ncccc");
        assert_ne!(c.value, 10001);
        let n = frame.numbers(&mut numbers);
        assert_eq!(&numbers[..n], b"\n");
    }
    #[test]
    fn rows_move_up_and_down_in_wrapped_lines() {
        let text = "aaaa bbbb cccc\nshort";
        let w = width("aaaa ", 16) + 2;
        let mut column = None;
        let down = motion_wrapped(text, 1, edit::DOWN, 16, 5, &mut column, w);
        assert_eq!(down, 6);
        let down = motion_wrapped(text, down, edit::DOWN, 16, 5, &mut column, w);
        assert_eq!(down, 11);
        let down = motion_wrapped(text, down, edit::DOWN, 16, 5, &mut column, w);
        assert_eq!(down, 16);
        let up = motion_wrapped(text, down, edit::UP, 16, 5, &mut column, w);
        assert_eq!(up, 11);
        let mut column = None;
        assert_eq!(
            motion_wrapped(text, 7, edit::HOME, 16, 5, &mut column, w),
            5
        );
        // End stays on the row: before the space the next row starts after.
        assert_eq!(motion_wrapped(text, 7, edit::END, 16, 5, &mut column, w), 9);
        assert_eq!(
            motion_wrapped(text, 11, edit::END, 16, 5, &mut column, w),
            14
        );
    }
    #[test]
    fn long_unwrapped_lines_get_a_horizontal_scrollbar() {
        let line = "x".repeat(300);
        let mut scroll = Scroll::default();
        let mut out = [0u8; 1024];
        let mut frame = Frame::EMPTY;
        let options = Options {
            wrap: false,
            longest: 300,
        };
        assert!(needs_horizontal_scrollbar(options, scroll, 16, 200));
        assert!(!needs_horizontal_scrollbar(
            Options {
                wrap: false,
                longest: 3
            },
            scroll,
            16,
            200
        ));
        let c = compose_with(
            &line,
            &mut scroll,
            0,
            0,
            false,
            16,
            200,
            40,
            &mut out,
            &mut frame,
            options,
        );
        assert_eq!(decode_horizontal_scroll(c.scroll_x), Some(0));
        let c = compose_with(
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
            options,
        );
        assert!(decode_horizontal_scroll(c.scroll_x).unwrap() > 9000);
        let g = geometry_with_scroll(
            Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            0,
            c.scroll_x,
        );
        let side = g.horizontal_track.unwrap();
        assert_eq!(side.bottom(), 100);
        assert_eq!(g.text.height, 100 - BAR - 2);
        assert_eq!(g.track.unwrap().height, g.text.height);
        assert_eq!(side.width, g.text.width);
        assert_eq!(g.horizontal_position(0), Some(0));
        assert_eq!(g.horizontal_position(1000), Some(10000));
        assert_eq!(
            left_for(300, 16, 200, 10000),
            horizontal_scroll_range(300, 16, 200)
        );
        assert_eq!(encode_horizontal_scroll(None), 0);
        assert_eq!(
            decode_horizontal_scroll(encode_horizontal_scroll(Some(10000))),
            Some(10000)
        );
        assert_eq!(
            decode_horizontal_scroll(encode_horizontal_scroll(Some(0))),
            Some(0)
        );
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
