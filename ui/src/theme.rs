//! Shared native-control palette. Compiled views retain token IDs, so selecting
//! a theme changes pixels without recompiling CSS or changing application data.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u32)]
pub enum Theme {
    #[default]
    Dark = 0,
    Contrast = 1,
}

impl Theme {
    pub const fn from_id(id: u32) -> Option<Self> {
        match id {
            0 => Some(Self::Dark),
            1 => Some(Self::Contrast),
            _ => None,
        }
    }
    /// Straight ARGB; text consumers discard alpha. Zero means a literal color.
    pub fn color(self, token: u8, literal: u32) -> u32 {
        if token == 0 || token > TOKEN_COUNT {
            return literal;
        }
        let palette = match self {
            Self::Dark => &DARK,
            Self::Contrast => &CONTRAST,
        };
        palette[usize::from(token - 1)]
    }
}

pub const TOKEN_COUNT: u8 = 14;
pub const TEXT: u8 = 1;
pub const SURFACE: u8 = 3;
pub const MUTED: u8 = 2;
pub const TRACK: u8 = 6;
pub const ACCENT: u8 = 8;
pub const GRID: u8 = 14;
const DARK: [u32; TOKEN_COUNT as usize] = [
    0xffe9edf6, 0xffaeb8cc, 0xff343d50, 0xff3a4357, 0x1cffffff, 0x1fffffff, 0xff4b4a80, 0xffa78bfa,
    0xff7aa7ff, 0xffff9f9f, 0xffffd66c, 0xff32384a, 0xff3e4760, 0xff4b5266,
];
const CONTRAST: [u32; TOKEN_COUNT as usize] = [
    0xffffffff, 0xffcccccc, 0xff101010, 0xff000000, 0xffffffff, 0xff555555, 0xff274e91, 0xffffff00,
    0xff80dfff, 0xffff8080, 0xffffe080, 0xff000000, 0xff202020, 0xff999999,
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
    ]
    .iter()
    .position(|candidate| *candidate == name)
    .map(|i| i as u8 + 1)
}

/// SDK origin: application rules override these defaults regardless of their
/// specificity. The System Manager is the reference for native control metrics.
#[cfg(feature = "compiler")]
pub(crate) const CONTROLS: &str = include_str!("controls.css");
