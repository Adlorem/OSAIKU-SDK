#![no_std]
#![forbid(unsafe_code)]

//! Bounded native UI. Compilation is optional and runs only in user space.
//! Runtime values remain data. Layout is the common source for paint/input.

#[cfg(feature = "compiler")]
extern crate alloc;
pub mod appearance;
pub mod catalog;
#[cfg(feature = "compiler")]
pub mod compiler;
pub mod context;
pub mod decoration;
pub mod editing;
pub mod font;
pub mod font_atlas;
pub mod image;
pub mod input;
pub mod layout;
pub mod paint;
pub mod raster;
pub mod report;
mod shape;
pub mod surface;
pub mod text_atlas;
pub mod text_model;
pub mod theme;
pub mod virtual_list;
mod wire;

pub const VERSION: u16 = 11;
pub const MAX_STATE_STYLES: usize = 128;
pub const NO_STATE_STYLE: u8 = u8::MAX;
pub const MAX_DECORATIONS: usize = 32;
/// `Decoration::background_image` of the host's first image set.
pub const HOST_IMAGE: u8 = 0x80;
/// Image sets a background can name.
pub const HOST_IMAGES: u8 = 8;
pub const NO_DECORATION: u8 = u8::MAX;
pub const MAX_NODES: usize = 256;
pub const MAX_DEPTH: usize = 24;
pub const MAX_TEXT: usize = 8192;
pub const MAX_LEAF_TEXT: usize = 256;
pub const MAX_POINTS: usize = 120;
pub const MAX_WIRE: usize = 65536;
pub const NONE: u16 = u16::MAX;

/// Digest of the exact versioned transport bytes, never an authority token.
/// A host validates the document and authenticates the submitting owner too.
pub fn wire_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    hash.max(1)
}

/// Stable names, scoped to a window incarnation. Compiler rejects collisions.
pub const fn symbol(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut hash = 2166136261u32;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u32).wrapping_mul(16777619);
        i += 1;
    }
    hash
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Container = 0,
    Text = 1,
    Button = 2,
    Table = 3,
    Row = 4,
    Cell = 5,
    Chart = 6,
    ValueBar = 7,
    Separator = 8,
    Icon = 9,
    TextInput = 10,
    VirtualList = 11,
    Template = 12,
    /// `ui-popup-menu`: a container drawn above the rest of the document at
    /// an anchor point, outside its parent's flow (see `Node::popup_anchor`).
    Popup = 13,
}
impl Kind {
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Container,
            1 => Self::Text,
            2 => Self::Button,
            3 => Self::Table,
            4 => Self::Row,
            5 => Self::Cell,
            6 => Self::Chart,
            7 => Self::ValueBar,
            8 => Self::Separator,
            9 => Self::Icon,
            10 => Self::TextInput,
            11 => Self::VirtualList,
            12 => Self::Template,
            13 => Self::Popup,
            _ => return None,
        })
    }
    pub fn container(self) -> bool {
        matches!(
            self,
            Self::Container
                | Self::Table
                | Self::Row
                | Self::VirtualList
                | Self::Template
                | Self::Popup
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Role {
    Group = 0,
    Text = 1,
    Heading = 2,
    Navigation = 3,
    Button = 4,
    Table = 5,
    Row = 6,
    ColumnHeader = 7,
    Cell = 8,
    Chart = 9,
    Meter = 10,
    Separator = 11,
    Presentation = 12,
    TextBox = 13,
}
impl Role {
    pub fn from_byte(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Group,
            1 => Self::Text,
            2 => Self::Heading,
            3 => Self::Navigation,
            4 => Self::Button,
            5 => Self::Table,
            6 => Self::Row,
            7 => Self::ColumnHeader,
            8 => Self::Cell,
            9 => Self::Chart,
            10 => Self::Meter,
            11 => Self::Separator,
            12 => Self::Presentation,
            13 => Self::TextBox,
            _ => return None,
        })
    }
    fn matches(self, kind: Kind) -> bool {
        match kind {
            Kind::Container | Kind::Popup => matches!(self, Self::Group | Self::Navigation),
            Kind::VirtualList | Kind::Template => self == Self::Group,
            Kind::Text => matches!(self, Self::Text | Self::Heading),
            Kind::Button => self == Self::Button,
            Kind::Table => self == Self::Table,
            Kind::Row => self == Self::Row,
            Kind::Cell => matches!(self, Self::Cell | Self::ColumnHeader),
            Kind::Chart => self == Self::Chart,
            Kind::ValueBar => self == Self::Meter,
            Kind::Separator => self == Self::Separator,
            Kind::Icon => self == Self::Presentation,
            Kind::TextInput => self == Self::TextBox,
        }
    }
}

pub mod state {
    pub const HIDDEN: u8 = 1;
    pub const DISABLED: u8 = 2;
    pub const SELECTED: u8 = 4;
    pub const FOCUSED: u8 = 8;
    pub const ALL: u8 = 15;
}

/// Percent widths are encoded as negative percentages; i16::MAX is auto.
/// u16::MAX denotes auto height; zero pixel dimensions retain CSS meaning.
/// All other dimensions are logical pixels. Text color is opaque RGB;
/// background is straight-alpha ARGB (CSS #RRGGBB or #RRGGBBAA).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    pub width: i16,
    pub height: u16,
    pub padding: u16,
    pub gap: u16,
    pub font_size: u16,
    pub color: u32,
    pub background: u32,
    /// Shared palette token IDs; zero retains the corresponding literal color.
    pub color_token: u8,
    pub background_token: u8,
    pub direction: u8,
    pub overflow: u8,
    pub fill: bool,
    pub grow: bool,
    /// Inherited horizontal text placement: 0 left, 1 center, 2 right.
    pub text_align: u8,
    /// Index into `Document::decorations`, or NO_DECORATION.
    pub decoration: u8,
}
impl Style {
    pub const DEFAULT: Self = Self {
        width: i16::MAX,
        height: u16::MAX,
        padding: 0,
        gap: 0,
        font_size: 16,
        color: 0xe9edf6,
        background: 0,
        color_token: 0,
        background_token: 0,
        direction: 0,
        overflow: 0,
        fill: false,
        grow: false,
        text_align: 0,
        decoration: NO_DECORATION,
    };
    pub fn valid(self) -> bool {
        ((-100..=4096).contains(&self.width) || self.width == i16::MAX)
            && (self.height <= 4096 || self.height == u16::MAX)
            && self.padding <= 256
            && self.gap <= 256
            && font::ui_text_size_supported(self.font_size)
            && self.color <= 0xffffff
            && self.color_token <= theme::TOKEN_COUNT
            && self.background_token <= theme::TOKEN_COUNT
            && self.direction <= 1
            && self.overflow <= 2
            && self.text_align <= 2
    }
}

/// Box decoration, interned in a bounded per-document table so that `Style`
/// and the retained recipe keep their size. NONE is never stored in the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoration {
    /// Uniform corner radius in pixels; painting clamps it to half the
    /// shorter side, so a large radius produces a pill or circle.
    pub radius: u8,
    /// Border width in pixels (0..=16), on the sides named in `border_sides`.
    pub border_width: u8,
    /// Bit 0 top, 1 right, 2 bottom, 3 left.
    pub border_sides: u8,
    /// Text placement inside the content box: 0 top, 1 middle, 2 bottom.
    pub vertical_align: u8,
    /// Vertical placement of a row's children: 0 start, 1 center, 2 end,
    /// 3 stretch (auto-height children fill the row).
    pub align_items: u8,
    /// Coverage of the area under a chart series (SVG fill-opacity); 0 is none.
    pub fill_opacity: u8,
    /// The border uses the resolved text color (CSS currentColor).
    pub border_current_color: bool,
    /// Straight ARGB border color, zero when currentColor applies.
    pub border_color: u32,
    pub border_token: u8,
    /// The focus indicator is not drawn (CSS `outline: none`); the element
    /// then shows focus through its own state rules.
    pub outline_none: bool,
    /// Background image, painted over the background color: zero is none,
    /// `n` the vertical gradient of `appearance::BACKGROUNDS[n - 1]`, and
    /// [`HOST_IMAGE`] + `i` the host's `i`-th image set, centred. A reference
    /// keeps the decoration in its 16 bytes.
    pub background_image: u8,
}
impl Decoration {
    pub const NONE: Self = Self {
        radius: 0,
        border_width: 0,
        border_sides: 0,
        vertical_align: 0,
        align_items: 0,
        fill_opacity: 0,
        border_current_color: false,
        border_color: 0,
        border_token: 0,
        outline_none: false,
        background_image: 0,
    };
    pub fn valid(self) -> bool {
        self.border_width <= 16
            && self.border_sides <= 15
            && (self.border_width == 0) == (self.border_sides == 0)
            && self.vertical_align <= 2
            && self.align_items <= 3
            && self.border_token <= theme::TOKEN_COUNT
            && (usize::from(self.background_image) <= appearance::BACKGROUNDS.len()
                || (HOST_IMAGE..HOST_IMAGE + HOST_IMAGES).contains(&self.background_image))
            && !(self.border_current_color && (self.border_color != 0 || self.border_token != 0))
    }
    /// Border color fields are meaningless without a border; one canonical
    /// form keeps equal decorations equal in the interned table.
    pub fn canonical(self) -> Self {
        if self.border_width == 0 {
            Self {
                border_current_color: false,
                border_color: 0,
                border_token: 0,
                ..self
            }
        } else {
            self
        }
    }
    /// Border widths in CSS order: top, right, bottom, left.
    pub fn border_widths(self) -> [u32; 4] {
        core::array::from_fn(|side| {
            if self.border_sides & (1 << side) != 0 {
                u32::from(self.border_width)
            } else {
                0
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: u32,
    pub parent: u16,
    pub kind: Kind,
    pub role: Role,
    pub state: u8,
    pub action: u32,
    pub text_start: u16,
    pub text_len: u16,
    pub bindings: [u32; 6],
    /// Percent value, the configuration index for Kind::VirtualList, or the
    /// anchor's x for Kind::Popup.
    pub value: u16,
    pub series_start: u16,
    pub series_len: u16,
    pub style: Style,
    pub narrow: Style,
    pub breakpoint: u16,
    /// Requested vertical offset, clamped by the shared layout; the anchor's
    /// y for Kind::Popup, which never scrolls.
    pub scroll_y: u32,
    /// Seven nonempty disabled/selected/focused combinations, wide then narrow.
    /// NO_STATE_STYLE reuses the corresponding base style.
    pub state_styles: [u8; 14],
    /// Per combination (including empty): bit 0 inherits color, bit 1 font
    /// size, bit 2 text alignment.
    pub inherited: [u8; 16],
}
impl Node {
    /// A popup's anchor in document (viewport) pixels: the point a context
    /// menu opens at. Layout keeps the popup inside the viewport, opening it
    /// to the left of or above the point when there is no room after it.
    pub fn popup_anchor(&self) -> Option<(u32, u32)> {
        (self.kind == Kind::Popup).then_some((u32::from(self.value), self.scroll_y))
    }
    /// Set a popup's anchor, clamped to the largest viewport (4096 px).
    pub fn set_popup_anchor(&mut self, x: u32, y: u32) {
        if self.kind == Kind::Popup {
            self.value = x.min(4096) as u16;
            self.scroll_y = y.min(4096);
        }
    }
    pub const EMPTY: Self = Self {
        id: 0,
        parent: NONE,
        kind: Kind::Container,
        role: Role::Group,
        state: 0,
        action: 0,
        text_start: 0,
        text_len: 0,
        bindings: [0; 6],
        value: 10001,
        series_start: 0,
        series_len: 0,
        style: Style::DEFAULT,
        narrow: Style::DEFAULT,
        breakpoint: 0,
        scroll_y: 0,
        state_styles: [NO_STATE_STYLE; 14],
        inherited: [0; 16],
    };
}

/// Timestamp in milliseconds relative to a measurement epoch. 10001 is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub time_ms: u64,
    /// Percent value, or the configuration index for Kind::VirtualList.
    pub value: u16,
}
impl Point {
    pub const EMPTY: Self = Self {
        time_ms: 0,
        value: 10001,
    };
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub nodes: [Node; MAX_NODES],
    pub len: u16,
    pub text: [u8; MAX_TEXT],
    pub text_len: u16,
    pub points: [Point; MAX_POINTS],
    pub point_len: u16,
    pub state_styles: [Style; MAX_STATE_STYLES],
    pub state_style_len: u16,
    pub decorations: [Decoration; MAX_DECORATIONS],
    pub decoration_len: u16,
    pub lists: [virtual_list::Config; virtual_list::MAX_LISTS],
    pub list_len: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Version,
    Limit,
    Structure,
    Text,
    Binding,
    Value,
    Buffer,
    Stale,
    Disabled,
    Owner,
}

pub enum Value<'a> {
    Text(&'a str),
    Percent(Option<u16>),
    Series(&'a [Point]),
    Bool(bool),
}

/// Bindings are resolved locally; function pointers never enter encoded data.
pub trait Bindings {
    fn get(&self, name: u32) -> Option<Value<'_>>;
}

impl Document {
    pub const EMPTY: Self = Self {
        nodes: [Node::EMPTY; MAX_NODES],
        len: 0,
        text: [0; MAX_TEXT],
        text_len: 0,
        points: [Point::EMPTY; MAX_POINTS],
        point_len: 0,
        state_styles: [Style::DEFAULT; MAX_STATE_STYLES],
        state_style_len: 0,
        decorations: [Decoration::NONE; MAX_DECORATIONS],
        decoration_len: 0,
        lists: [virtual_list::Config::EMPTY; virtual_list::MAX_LISTS],
        list_len: 0,
    };
    pub fn validate(&self) -> Result<(), Error> {
        let len = self.len as usize;
        if len == 0
            || len > MAX_NODES
            || self.text_len as usize > MAX_TEXT
            || self.point_len as usize > MAX_POINTS
            || self.state_style_len as usize > MAX_STATE_STYLES
            || self.decoration_len as usize > MAX_DECORATIONS
            || self.list_len as usize > virtual_list::MAX_LISTS
        {
            return Err(Error::Limit);
        }
        let style_valid = |s: &Style| {
            s.valid()
                && (s.decoration == NO_DECORATION || u16::from(s.decoration) < self.decoration_len)
        };
        if self.state_styles[..self.state_style_len as usize]
            .iter()
            .any(|s| !style_valid(s))
            || self.decorations[..self.decoration_len as usize]
                .iter()
                .any(|d| !d.valid() || *d != d.canonical() || *d == Decoration::NONE)
        {
            return Err(Error::Structure);
        }
        core::str::from_utf8(&self.text[..self.text_len as usize]).map_err(|_| Error::Text)?;
        for (i, n) in self.nodes[..len].iter().enumerate() {
            if n.id == 0
                || self.nodes[..i].iter().any(|p| p.id == n.id)
                || !n.role.matches(n.kind)
                || n.state & !state::ALL != 0
                || !style_valid(&n.style)
                || !style_valid(&n.narrow)
                || n.breakpoint > 4096
                || n.value > 10001
                || n.scroll_y > 1_048_576
                || n.state_styles.iter().any(|index| {
                    *index != NO_STATE_STYLE && usize::from(*index) >= self.state_style_len as usize
                })
                || n.inherited.iter().any(|flags| *flags & !7 != 0)
            {
                return Err(Error::Structure);
            }
            if (i == 0 && n.parent != NONE)
                || (i != 0
                    && (n.parent as usize >= i || !self.nodes[n.parent as usize].kind.container()))
            {
                return Err(Error::Structure);
            }
            let mut parent = n.parent;
            let mut depth = 1;
            while parent != NONE {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(Error::Limit);
                }
                parent = self.nodes[parent as usize].parent;
            }
            if n.action != 0 && !catalog::actionable(n.kind) {
                return Err(Error::Structure);
            }
            if n.text_len as usize > MAX_LEAF_TEXT
                || n.text_start as usize + n.text_len as usize > self.text_len as usize
            {
                return Err(Error::Text);
            }
            core::str::from_utf8(
                &self.text[n.text_start as usize..n.text_start as usize + n.text_len as usize],
            )
            .map_err(|_| Error::Text)?;
            if n.series_len > 60
                || n.series_start as usize + n.series_len as usize > self.point_len as usize
                || (n.series_len != 0 && n.kind != Kind::Chart)
            {
                return Err(Error::Value);
            }
            let points =
                &self.points[n.series_start as usize..(n.series_start + n.series_len) as usize];
            if points.iter().any(|p| p.value > 10001)
                || points.windows(2).any(|p| p[0].time_ms >= p[1].time_ms)
            {
                return Err(Error::Value);
            }
            if catalog::BINDINGS
                .iter()
                .any(|b| n.bindings[b.slot] != 0 && !b.accepts(n.kind))
            {
                return Err(Error::Binding);
            }
            // A popup is positioned by its anchor, never by a list row.
            if n.kind == Kind::Popup
                && (n.value > 4096
                    || n.scroll_y > 4096
                    || n.style.overflow == 2
                    || n.narrow.overflow == 2
                    || virtual_list::in_template(self, i))
            {
                return Err(Error::Value);
            }
        }
        virtual_list::validate(self)?;
        Ok(())
    }
    /// Resolve a validated node's state and inherited text properties along a
    /// bounded ancestor path. The same result drives layout and paint.
    pub(crate) fn resolved_style(&self, index: usize, width: u32) -> Style {
        let mut path = [0usize; MAX_DEPTH];
        let mut depth = 0;
        let mut current = index;
        loop {
            path[depth] = current;
            depth += 1;
            if self.nodes[current].parent == NONE {
                break;
            }
            current = usize::from(self.nodes[current].parent);
        }
        let mut inherited_style = Style::DEFAULT;
        let mut disabled = false;
        for level in (0..depth).rev() {
            let n = &self.nodes[path[level]];
            disabled |= n.state & state::DISABLED != 0;
            let bits = n.state | if disabled { state::DISABLED } else { 0 };
            let combination = usize::from((bits >> 1) & 7);
            let narrow = usize::from(n.breakpoint != 0 && width <= u32::from(n.breakpoint));
            let mut style = if narrow != 0 { n.narrow } else { n.style };
            if combination != 0 {
                let entry = n.state_styles[narrow * 7 + combination - 1];
                if entry != NO_STATE_STYLE {
                    style = self.state_styles[usize::from(entry)];
                }
            }
            let flags = n.inherited[narrow * 8 + combination];
            if flags & 1 != 0 {
                style.color = inherited_style.color;
                style.color_token = inherited_style.color_token;
            }
            if flags & 2 != 0 {
                style.font_size = inherited_style.font_size;
            }
            if flags & 4 != 0 {
                style.text_align = inherited_style.text_align;
            }
            inherited_style = style;
        }
        inherited_style
    }
    /// The decoration a resolved style names; NONE when it names none.
    pub fn decoration(&self, style: Style) -> Decoration {
        self.decorations[..usize::from(self.decoration_len).min(MAX_DECORATIONS)]
            .get(usize::from(style.decoration))
            .copied()
            .unwrap_or(Decoration::NONE)
    }
    /// Padding plus border, in CSS order: top, right, bottom, left. Sizes
    /// include both (border-box), so the content box is the remainder.
    pub fn insets(&self, style: Style) -> [u32; 4] {
        self.decoration(style)
            .border_widths()
            .map(|border| border + u32::from(style.padding))
    }
    /// Replace focus with one actionable, enabled, visible node. Hidden and
    /// disabled ancestors suppress focus too. Zero/missing IDs clear it.
    /// This checks only focus eligibility, not full document validity or viewport
    /// clipping; validate/encode and layout still establish those separately.
    /// Malformed parent paths are bounded and cannot publish focus.
    pub fn set_focus(&mut self, id: u32) -> bool {
        for node in &mut self.nodes {
            node.state &= !state::FOCUSED;
        }
        let len = usize::from(self.len);
        if len == 0 || len > MAX_NODES || id == 0 {
            return false;
        }
        let Some(index) = self.nodes[..len]
            .iter()
            .position(|node| node.id == id && node.action != 0)
        else {
            return false;
        };
        let mut ancestor = index;
        for _ in 0..MAX_DEPTH {
            let node = &self.nodes[ancestor];
            if node.state & (state::DISABLED | state::HIDDEN) != 0 {
                return false;
            }
            if node.parent == NONE {
                if ancestor != 0 {
                    return false;
                }
                self.nodes[index].state |= state::FOCUSED;
                return true;
            }
            let parent = usize::from(node.parent);
            if parent >= ancestor || !self.nodes[parent].kind.container() {
                return false;
            }
            ancestor = parent;
        }
        false
    }

    pub fn text_of(&self, index: usize) -> &str {
        let n = &self.nodes[index];
        core::str::from_utf8(
            &self.text[n.text_start as usize..n.text_start as usize + n.text_len as usize],
        )
        .unwrap_or("")
    }
    /// Prepares a replacement in caller-owned scratch storage. On error the
    /// source stays valid; scratch is unspecified and must not be published.
    /// No Document-sized temporary is placed on the runtime stack.
    pub fn bind_into(&self, source: &impl Bindings, next: &mut Self) -> Result<(), Error> {
        self.validate()?;
        next.nodes.copy_from_slice(&self.nodes);
        next.state_styles.copy_from_slice(&self.state_styles);
        next.state_style_len = self.state_style_len;
        next.decorations.copy_from_slice(&self.decorations);
        next.decoration_len = self.decoration_len;
        next.lists.copy_from_slice(&self.lists);
        next.list_len = self.list_len;
        next.len = self.len;
        next.text_len = 0;
        next.point_len = 0;
        next.text.fill(0);
        next.points.fill(Point::EMPTY);
        for i in 0..self.len as usize {
            let old = &self.nodes[i];
            let n = &mut next.nodes[i];
            let prototype = virtual_list::in_template(self, i);
            let text = if old.bindings[0] == 0 || prototype {
                self.text_of(i)
            } else {
                match source.get(old.bindings[0]) {
                    Some(Value::Text(v)) => v,
                    _ => return Err(Error::Binding),
                }
            };
            if text.len() > MAX_LEAF_TEXT || next.text_len as usize + text.len() > MAX_TEXT {
                return Err(Error::Limit);
            }
            n.text_start = next.text_len;
            n.text_len = text.len() as u16;
            next.text[next.text_len as usize..next.text_len as usize + text.len()]
                .copy_from_slice(text.as_bytes());
            next.text_len += text.len() as u16;
            if old.bindings[1] != 0 && !prototype {
                n.value = match source.get(old.bindings[1]) {
                    Some(Value::Percent(v)) => v.unwrap_or(10001),
                    _ => return Err(Error::Binding),
                };
            }
            let series = if old.bindings[2] == 0 || prototype {
                &self.points
                    [old.series_start as usize..(old.series_start + old.series_len) as usize]
            } else {
                match source.get(old.bindings[2]) {
                    Some(Value::Series(v)) => v,
                    _ => return Err(Error::Binding),
                }
            };
            if series.len() > 60 || next.point_len as usize + series.len() > MAX_POINTS {
                return Err(Error::Limit);
            }
            n.series_start = next.point_len;
            n.series_len = series.len() as u16;
            next.points[next.point_len as usize..next.point_len as usize + series.len()]
                .copy_from_slice(series);
            next.point_len += series.len() as u16;
            for (slot, flag) in [
                (3, state::HIDDEN),
                (4, state::DISABLED),
                (5, state::SELECTED),
            ] {
                if old.bindings[slot] != 0 && !prototype {
                    match source.get(old.bindings[slot]) {
                        Some(Value::Bool(v)) => {
                            n.state = (n.state & !flag) | if v { flag } else { 0 }
                        }
                        _ => return Err(Error::Binding),
                    }
                }
            }
        }
        next.validate()?;
        Ok(())
    }
}

/// No address, callback, process-private type or authority token is encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Identity {
    /// Compositor lifetime shared by immutable pixels, semantics and input.
    pub epoch: u64,
    pub owner: u64,
    pub window: u64,
    pub incarnation: u64,
    pub revision: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Action {
    pub target: Identity,
    pub element: u32,
    pub action: u32,
}
const _: () = assert!(core::mem::size_of::<Identity>() == 40);
const _: () = assert!(core::mem::size_of::<Action>() == 48);
impl Action {
    pub const EMPTY: Self = Self {
        target: Identity {
            epoch: 0,
            owner: 0,
            window: 0,
            incarnation: 0,
            revision: 0,
        },
        element: 0,
        action: 0,
    };
    /// Call only against the actual presented recipe. Admission is insufficient.
    pub fn validate(self, presented: Identity, view: &Document) -> Result<(), Error> {
        view.validate()?;
        if presented.owner == 0
            || presented.window == 0
            || presented.incarnation == 0
            || self.target.owner != presented.owner
            || self.target.window != presented.window
            || self.target.incarnation != presented.incarnation
        {
            return Err(Error::Owner);
        }
        if presented.epoch == 0
            || self.target.epoch != presented.epoch
            || presented.revision == 0
            || self.target.revision != presented.revision
        {
            return Err(Error::Stale);
        }
        let n = view.nodes[..view.len as usize]
            .iter()
            .find(|n| n.id == self.element)
            .ok_or(Error::Stale)?;
        let mut current = n;
        loop {
            if current.state & (state::DISABLED | state::HIDDEN) != 0 {
                return Err(Error::Disabled);
            }
            if current.parent == NONE {
                break;
            }
            current = &view.nodes[current.parent as usize];
        }
        if self.action == 0 || n.action != self.action {
            return Err(Error::Stale);
        }
        Ok(())
    }
}
