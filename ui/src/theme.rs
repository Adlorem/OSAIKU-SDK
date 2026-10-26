//! Shared native-control palette. Compiled views retain token IDs, so selecting
//! a theme changes pixels without recompiling CSS or changing application data.
use crate::appearance;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum Theme {
    #[default]
    Dark = 0,
    Contrast = 1,
    /// Warm paper with brown ink: `appearance::Base::Light`.
    Light = 2,
}

impl Theme {
    pub const fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::Dark),
            1 => Some(Self::Contrast),
            2 => Some(Self::Light),
            _ => None,
        }
    }
    /// The palette of a desktop base theme.
    pub const fn for_base(base: appearance::Base) -> Self {
        match base {
            appearance::Base::Dark => Self::Dark,
            appearance::Base::Light => Self::Light,
        }
    }
    /// The palette with accent `accent` (an `appearance::ACCENTS` index) in
    /// place of the one its base theme comes with; `None` keeps that one.
    /// The high-contrast palette keeps its own.
    pub fn accented(self, token: u8, literal: u32, accent: Option<usize>) -> u32 {
        let base = match self {
            Self::Dark => 0,
            Self::Light => 1,
            Self::Contrast => return self.color(token, literal),
        };
        match (accent.and_then(|a| appearance::ACCENTS.get(a)), token) {
            (Some(a), SELECTED) => OPAQUE | a.selected[base],
            (Some(a), ACCENT) => OPAQUE | a.token[base],
            (Some(a), ACCENT_FILL) => OPAQUE | a.fill,
            _ => self.color(token, literal),
        }
    }
    /// The id a host transports: the palette in the low byte, the chosen
    /// accent + 1 in the next (0: the base theme's own).
    pub const fn id_with_accent(self, accent: Option<usize>) -> u32 {
        let accent = match accent {
            Some(a) if a < appearance::ACCENTS.len() => a as u32 + 1,
            _ => 0,
        };
        self as u32 | accent << 8
    }
    /// A transported id: the palette and the accent, if one is chosen.
    pub const fn decode(id: u32) -> Option<(Self, Option<usize>)> {
        let Some(theme) = Self::from_id(id & 0xff) else {
            return None;
        };
        let accent = (id >> 8) as usize;
        if id >> 16 != 0 || accent > appearance::ACCENTS.len() {
            return None;
        }
        Some((theme, if accent == 0 { None } else { Some(accent - 1) }))
    }
    /// Straight ARGB; text consumers discard alpha. Zero means a literal color.
    pub fn color(self, token: u8, literal: u32) -> u32 {
        if token == 0 || token > TOKEN_COUNT {
            return literal;
        }
        let palette = match self {
            Self::Dark => &DARK,
            Self::Contrast => &CONTRAST,
            Self::Light => &LIGHT,
        };
        palette[usize::from(token - 1)]
    }
}

pub const TOKEN_COUNT: u8 = 15;
pub const TEXT: u8 = 1;
pub const SURFACE: u8 = 3;
pub const MUTED: u8 = 2;
pub const TRACK: u8 = 6;
pub const SELECTED: u8 = 7;
pub const ACCENT: u8 = 8;
pub const GRID: u8 = 14;
/// The accent's own fill, under white text: the shell's buttons and marks.
pub const ACCENT_FILL: u8 = 15;
const OPAQUE: u32 = 0xff00_0000;
const DARK: [u32; TOKEN_COUNT as usize] = [
    0xffe9edf6,
    0xffaeb8cc,
    OPAQUE | appearance::DARK.surface,
    0xff3a4357,
    0x1cffffff,
    0x1fffffff,
    OPAQUE | appearance::ACCENTS[0].selected[0],
    OPAQUE | appearance::ACCENTS[0].token[0],
    0xff7aa7ff,
    0xffff9f9f,
    0xffffd66c,
    0xff32384a,
    0xff3e4760,
    0xff4b5266,
    OPAQUE | appearance::ACCENTS[0].fill,
];
const LIGHT: [u32; TOKEN_COUNT as usize] = [
    OPAQUE | appearance::LIGHT.text,
    0xff7b6e64,
    OPAQUE | appearance::LIGHT.surface,
    0xfffffdfa,
    0x24382818,
    0x1f382818,
    OPAQUE | appearance::ACCENTS[3].selected[1],
    OPAQUE | appearance::ACCENTS[3].token[1],
    0xff2f6db5,
    0xffc23b2e,
    0xffa86a00,
    0xfffffdfa,
    0xffffffff,
    0xffe2d6ca,
    OPAQUE | appearance::ACCENTS[3].fill,
];
const CONTRAST: [u32; TOKEN_COUNT as usize] = [
    0xffffffff, 0xffcccccc, 0xff101010, 0xff000000, 0xffffffff, 0xff555555, 0xff274e91, 0xffffff00,
    0xff80dfff, 0xffff8080, 0xffffe080, 0xff000000, 0xff202020, 0xff999999, 0xff274e91,
];

#[cfg(feature = "compiler")]
pub(crate) fn token(value: &str) -> Option<u8> {
    let name = value.strip_prefix("theme(")?.strip_suffix(')')?.trim();
    [
        "text",
        "muted",
        "surface",
        "card",
        "line",
        "track",
        "selected",
        "accent",
        "secondary",
        "danger",
        "warning",
        "input",
        "input-focus",
        "grid",
        "accent-fill",
    ]
    .iter()
    .position(|candidate| *candidate == name)
    .map(|i| i as u8 + 1)
}

/// SDK origin: application rules override these defaults regardless of their
/// specificity. The System Manager is the reference for native control metrics.
#[cfg(feature = "compiler")]
pub(crate) const CONTROLS: &str = include_str!("controls.css");
