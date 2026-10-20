//! Shared appearance presets: the one source of shell, window-decoration,
//! desktop-background and preview colours. Data and lookups only; choosing
//! an appearance and applying it belong to the host.

/// Base theme of the desktop chrome.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Base {
    #[default]
    Dark,
    Light,
}

impl Base {
    /// The index a host stores and transports: 0 Dark, 1 Light.
    pub const fn index(self) -> u32 {
        match self {
            Self::Dark => 0,
            Self::Light => 1,
        }
    }
    pub const fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Dark),
            1 => Some(Self::Light),
            _ => None,
        }
    }
    /// The `theme::Theme` id applications paint with.
    pub const fn theme_id(self) -> u32 {
        match self {
            Self::Dark => 0,
            Self::Light => 2,
        }
    }
    pub const fn chrome(self) -> &'static Chrome {
        match self {
            Self::Dark => &DARK,
            Self::Light => &LIGHT,
        }
    }
    /// The accent a base theme comes with: Fiolet, or Brąz for the warm
    /// light theme.
    pub const fn accent(self) -> usize {
        match self {
            Self::Dark => 0,
            Self::Light => 3,
        }
    }
    /// The background a base theme comes with: Góry, or Origami.
    pub const fn background(self) -> usize {
        match self {
            Self::Dark => 1,
            Self::Light => 3,
        }
    }
}

/// Shell and window-chrome colours (RGB unless noted) for one base theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chrome {
    /// Window content and start-menu buttons.
    pub surface: u32,
    /// Window title bar and start menu.
    pub title: u32,
    pub text: u32,
    pub border: u32,
    pub focus: u32,
    /// Halo of the focused window, blended over its underlay.
    pub glow: u32,
    /// Taskbar shelf before its opacity is applied.
    pub shelf: u32,
    /// Disabled and secondary shell text.
    pub muted: u32,
    /// Outlines of the shelf, start menu and its buttons; RGBA (CSS order).
    pub edge: u32,
    /// Rules between start-menu sections; RGBA (CSS order).
    pub rule: u32,
    /// Start-menu detail lines (diagnostics).
    pub detail: u32,
    /// The edge of the start menu's tail.
    pub tail: u32,
    /// Text of a disabled confirmation button, on the accent's dim fill.
    pub idle_text: u32,
    /// The assistant's mark, drawn on the shelf without a tile of its own.
    pub mark: u32,
}

pub const DARK: Chrome = Chrome {
    surface: 0x343d50,
    title: 0x2d3548,
    text: 0xe6edf7,
    border: 0x606578,
    focus: 0x7b7c91,
    glow: 0x7169a8,
    shelf: 0x222838,
    muted: 0x8b91a0,
    edge: 0xffffff1f,
    rule: 0xffffff1a,
    detail: 0xffd78a,
    tail: 0x474e5e,
    idle_text: 0xb8bdd0,
    mark: 0xffffff,
};

/// Warm paper: cream surfaces, brown ink and a brown accent (Brąz).
pub const LIGHT: Chrome = Chrome {
    surface: 0xfbf8f4,
    title: 0xf3ece4,
    text: 0x2a221d,
    border: 0xd3c4b5,
    focus: 0xe4d8cb,
    glow: 0xb08a64,
    shelf: 0xf8f2eb,
    muted: 0x8a7d72,
    edge: 0x3a2a1a24,
    rule: 0x3a2a1a1c,
    detail: 0x9a5b12,
    tail: 0xd3c4b5,
    idle_text: 0x9a8a7c,
    mark: 0x2a221d,
};

/// One accent is a set: the shell fill under white text (at least 4.5:1),
/// and per base theme the SDK `accent` token (at least 3:1 on the surface)
/// and the dim `selected` fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Accent {
    pub fill: u32,
    /// Dark, then Light.
    pub token: [u32; 2],
    pub selected: [u32; 2],
}

pub const ACCENTS: [Accent; 4] = [
    // Fiolet
    Accent {
        fill: 0x6654ce,
        token: [0xa78bfa, 0x5b45c9],
        selected: [0x4b4a80, 0xdcd5fa],
    },
    // Niebieski
    Accent {
        fill: 0x2a6bd0,
        token: [0x7aa7ff, 0x2860c4],
        selected: [0x2f4c80, 0xd3e1fa],
    },
    // Morski
    Accent {
        fill: 0x1f8073,
        token: [0x5fd0bd, 0x14776a],
        selected: [0x24585a, 0xcdeee8],
    },
    // Brąz, the light theme's own
    Accent {
        fill: 0x9b6b3f,
        token: [0xd9a877, 0x8a5a30],
        selected: [0x5a4632, 0xebddcf],
    },
];

/// A desktop background: a vertical gradient, solid when both stops match,
/// or a photograph built into the system ([`photo`]). A photograph's
/// gradient approximates it wherever the photograph itself is not painted: in
/// views that name the preset, and on the desktop until it has been decoded.
/// Views name one with `background-image: theme(name)`; the table order is
/// part of the wire format (a decoration stores `index + 1`), so entries keep
/// their index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Background {
    pub name: &'static str,
    pub top: u32,
    pub bottom: u32,
    /// [`photo`] has this preset's photograph.
    pub photo: bool,
}

pub const BACKGROUNDS: [Background; 4] = [
    // Granat
    Background {
        name: "navy",
        top: 0x0a0e1a,
        bottom: 0x0a0e1a,
        photo: false,
    },
    // Góry: mountains over a lake at dusk. The stops are the mean colours of
    // the photograph's top and bottom tenths.
    Background {
        name: "mountains",
        top: 0x7576c3,
        bottom: 0x695e9e,
        photo: true,
    },
    // Zmierzch
    Background {
        name: "dusk",
        top: 0x22357a,
        bottom: 0x7a4fd0,
        photo: false,
    },
    // Origami: a paper bear in morning light.
    Background {
        name: "origami",
        top: 0xeedfd5,
        bottom: 0xe7d3c4,
        photo: true,
    },
];

/// The photograph of preset `preset`: a baseline JPEG of 2132x1200 (16:9 at
/// the height of a 1920x1200 screen), for a host that decodes it. Kept out
/// of [`BACKGROUNDS`] so only a caller of this function links the files.
pub fn photo(preset: usize) -> Option<&'static [u8]> {
    match BACKGROUNDS.get(preset)?.name {
        "mountains" => Some(include_bytes!("../wallpapers/gory.jpg")),
        "origami" => Some(include_bytes!("../wallpapers/origami.jpg")),
        _ => None,
    }
}

/// Taskbar transparency steps in percent, as the user chooses them; the
/// shelf is drawn with the remaining opacity.
pub const SHELF_TRANSPARENCY: [u8; 4] = [0, 25, 50, 75];

/// A complete, valid selection. The default is the dark theme over the
/// mountain photograph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    base: Base,
    accent: u8,
    background: u8,
    shelf: u8,
}

impl Appearance {
    pub const DEFAULT: Self = Self {
        base: Base::Dark,
        accent: 0,
        background: 1,
        shelf: 0,
    };
    /// None when an index is outside its table.
    pub const fn new(base: Base, accent: usize, background: usize, shelf: usize) -> Option<Self> {
        if accent >= ACCENTS.len()
            || background >= BACKGROUNDS.len()
            || shelf >= SHELF_TRANSPARENCY.len()
        {
            return None;
        }
        Some(Self {
            base,
            accent: accent as u8,
            background: background as u8,
            shelf: shelf as u8,
        })
    }
    pub const fn base(self) -> Base {
        self.base
    }
    pub const fn accent(self) -> usize {
        self.accent as usize
    }
    pub const fn background(self) -> usize {
        self.background as usize
    }
    /// Index into [`SHELF_TRANSPARENCY`].
    pub const fn shelf(self) -> usize {
        self.shelf as usize
    }
    pub const fn chrome(self) -> &'static Chrome {
        self.base.chrome()
    }
    pub const fn accent_colors(self) -> &'static Accent {
        &ACCENTS[self.accent as usize]
    }
    pub const fn background_colors(self) -> &'static Background {
        &BACKGROUNDS[self.background as usize]
    }
    /// The shelf's coverage, 0..=255, rounded from its opacity.
    pub const fn shelf_alpha(self) -> u8 {
        shelf_alpha(SHELF_TRANSPARENCY[self.shelf as usize])
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Coverage of a shelf drawn at `transparency` percent (clamped to 100).
pub const fn shelf_alpha(transparency: u8) -> u8 {
    let opacity = 100
        - if transparency > 100 {
            100
        } else {
            transparency
        } as u32;
    ((opacity * 255 + 50) / 100) as u8
}
