//! Premultiplied sRGB pixels and linear-light shape coverage.
/// Straight foreground coverage becomes a private premultiplied cache pixel.
pub fn premultiply(color: u32, alpha: u8) -> u32 {
    blend_declared_xrgb(color, 0, alpha) | (u32::from(alpha) << 24)
}

/// Source-over for private premultiplied cache pixels, used by the frame ring.
pub fn foreground_over(color: u32, alpha: u8, behind: u32) -> u32 {
    let front = premultiply(color, alpha);
    let remainder = blend_declared_xrgb(behind, 0, 255 - alpha);
    let opacity = u32::from(alpha) + (((behind >> 24) * u32::from(255 - alpha) + 127) / 255);
    ((front & 0x00ff_ffff) + remainder) | (opacity << 24)
}

/// Linear light for each encoded channel value, scaled to `u16::MAX`.
///
/// Generated from the sRGB transfer function rather than approximated with a
/// power of 2.2: the curve has a straight segment near black, and this palette
/// spends most of its range there.
#[rustfmt::skip]
static SRGB_TO_LINEAR: [u16; 256] = [
    0x0000,0x0014,0x0028,0x003c,0x0050,0x0063,0x0077,0x008b,
    0x009f,0x00b3,0x00c7,0x00db,0x00f1,0x0108,0x0120,0x0139,
    0x0154,0x016f,0x018c,0x01ab,0x01ca,0x01eb,0x020e,0x0232,
    0x0257,0x027d,0x02a5,0x02ce,0x02f9,0x0325,0x0353,0x0382,
    0x03b3,0x03e5,0x0418,0x044d,0x0484,0x04bc,0x04f6,0x0532,
    0x056f,0x05ad,0x05ed,0x062f,0x0673,0x06b8,0x06fe,0x0747,
    0x0791,0x07dd,0x082a,0x087a,0x08ca,0x091d,0x0972,0x09c8,
    0x0a20,0x0a79,0x0ad5,0x0b32,0x0b91,0x0bf2,0x0c55,0x0cba,
    0x0d20,0x0d88,0x0df2,0x0e5e,0x0ecc,0x0f3c,0x0fae,0x1021,
    0x1097,0x110e,0x1188,0x1203,0x1280,0x1300,0x1381,0x1404,
    0x1489,0x1510,0x159a,0x1625,0x16b2,0x1741,0x17d3,0x1866,
    0x18fb,0x1993,0x1a2c,0x1ac8,0x1b66,0x1c06,0x1ca7,0x1d4c,
    0x1df2,0x1e9a,0x1f44,0x1ff1,0x20a0,0x2150,0x2204,0x22b9,
    0x2370,0x242a,0x24e5,0x25a3,0x2664,0x2726,0x27eb,0x28b1,
    0x297b,0x2a46,0x2b14,0x2be3,0x2cb6,0x2d8a,0x2e61,0x2f3a,
    0x3015,0x30f2,0x31d2,0x32b4,0x3399,0x3480,0x3569,0x3655,
    0x3742,0x3833,0x3925,0x3a1a,0x3b12,0x3c0b,0x3d07,0x3e06,
    0x3f07,0x400a,0x4110,0x4218,0x4323,0x4430,0x453f,0x4651,
    0x4765,0x487c,0x4995,0x4ab1,0x4bcf,0x4cf0,0x4e13,0x4f39,
    0x5061,0x518c,0x52b9,0x53e9,0x551b,0x5650,0x5787,0x58c1,
    0x59fe,0x5b3d,0x5c7e,0x5dc2,0x5f09,0x6052,0x619e,0x62ed,
    0x643e,0x6591,0x66e8,0x6840,0x699c,0x6afa,0x6c5b,0x6dbe,
    0x6f24,0x708d,0x71f8,0x7366,0x74d7,0x764a,0x77c0,0x7939,
    0x7ab4,0x7c32,0x7db3,0x7f37,0x80bd,0x8246,0x83d1,0x855f,
    0x86f0,0x8884,0x8a1b,0x8bb4,0x8d50,0x8eef,0x9090,0x9235,
    0x93dc,0x9586,0x9732,0x98e2,0x9a94,0x9c49,0x9e01,0x9fbb,
    0xa179,0xa339,0xa4fc,0xa6c2,0xa88b,0xaa56,0xac25,0xadf6,
    0xafca,0xb1a1,0xb37b,0xb557,0xb737,0xb919,0xbaff,0xbce7,
    0xbed2,0xc0c0,0xc2b1,0xc4a5,0xc69c,0xc895,0xca92,0xcc91,
    0xce94,0xd099,0xd2a1,0xd4ad,0xd6bb,0xd8cc,0xdae0,0xdcf7,
    0xdf11,0xe12e,0xe34e,0xe571,0xe797,0xe9c0,0xebec,0xee1b,
    0xf04d,0xf282,0xf4ba,0xf6f5,0xf933,0xfb74,0xfdb8,0xffff,
];

/// The encoded value nearest a linear one, by searching the encode table.
///
/// A reverse table would be four kilobytes of literal for the same answer; the
/// search is eight comparisons and is exact to the nearer neighbour.
const fn linear_to_srgb(linear: u32) -> u8 {
    let mut low = 0usize;
    let mut high = 255usize;
    while low < high {
        let mid = (low + high + 1) / 2;
        if SRGB_TO_LINEAR[mid] as u32 <= linear {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    if low < 255 {
        let below = SRGB_TO_LINEAR[low] as u32;
        let above = SRGB_TO_LINEAR[low + 1] as u32;
        if linear.saturating_sub(below) > above.saturating_sub(linear) {
            return (low + 1) as u8;
        }
    }
    low as u8
}

/// Blend a shape's coverage in linear light.
///
/// Coverage from geometry is a fraction of area, so half coverage must let
/// through half the light. Interpolating the encoded values instead makes that
/// pixel darker than half, which is why an unmanaged anti-aliased edge looks
/// pinched and why a light shape on a dark ground grows a dark seam.
///
/// Glyph coverage deliberately does not come through here. A font rasteriser's
/// coverage is tuned for perceptual weight in the space it expects to be
/// blended in; routing it through linear light thins light-on-dark text, which
/// is the opposite of what the tuning intends.
pub fn blend_shape_xrgb(foreground: u32, background: u32, coverage: u8) -> u32 {
    let mut result = 0u32;
    let mut shift = 0;
    while shift <= 16 {
        result |= u32::from(blend_shape_channel(
            (foreground >> shift) as u8,
            (background >> shift) as u8,
            coverage,
        )) << shift;
        shift += 8;
    }
    result
}

pub const fn blend_shape_channel(foreground: u8, background: u8, coverage: u8) -> u8 {
    if coverage == 0 || foreground == background {
        return background;
    }
    if coverage == 255 {
        return foreground;
    }
    let front = SRGB_TO_LINEAR[foreground as usize] as u32;
    let back = SRGB_TO_LINEAR[background as usize] as u32;
    let coverage = coverage as u32;
    linear_to_srgb((front * coverage + back * (255 - coverage) + 127) / 255)
}

pub const fn blend_declared_xrgb(foreground: u32, background: u32, coverage: u8) -> u32 {
    let red = blend_declared_channel((foreground >> 16) as u8, (background >> 16) as u8, coverage);
    let green = blend_declared_channel((foreground >> 8) as u8, (background >> 8) as u8, coverage);
    let blue = blend_declared_channel(foreground as u8, background as u8, coverage);
    ((red as u32) << 16) | ((green as u32) << 8) | blue as u32
}

const fn blend_declared_channel(foreground: u8, background: u8, coverage: u8) -> u8 {
    let coverage = coverage as u32;
    ((foreground as u32 * coverage + background as u32 * (255 - coverage) + 127) / 255) as u8
}
