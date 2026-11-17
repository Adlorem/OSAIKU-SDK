#[path = "driver_fixture.rs"]
pub mod driver;
use driver::*;
use osaiku_package::{digest, driver::GPU_CLASS, Domain};
pub const MATCH: usize = 192 + 592;
pub const GPU: usize = MATCH + 32;
pub const TABLE: usize = GPU + 192;

pub fn package(family: &str, device: u16, revision: (u8, u8)) -> Vec<u8> {
    let metadata = 592 + 32 + 192 + 224;
    let mut b = vec![0; 192 + metadata + 128];
    b[..8].copy_from_slice(Domain::Driver.magic());
    p32(&mut b, 8, 1);
    p32(&mut b, 12, 192);
    p64(&mut b, 24, 2);
    b[32..64].copy_from_slice(&publisher());
    let m = &mut b[192..192 + metadata];
    m[..8].copy_from_slice(b"OSADMF01");
    p32(m, 8, 2);
    p32(m, 12, metadata as u32);
    p64(m, 16, 1);
    p64(m, 32, 262144);
    p64(m, 40, 65536);
    p16(m, 48, 1);
    p16(m, 50, 1);
    p16(m, 54, 1);
    p16(m, 60, 80);
    p16(m, 62, 2);
    m[64..64 + family.len()].copy_from_slice(family.as_bytes());
    m[192..195].copy_from_slice(b"GPU");
    m[320] = b'1';
    m[384] = b'1';
    p64(m, 448, GPU_CLASS);
    p32(m, 456, 50);
    p16(m, 464, 8);
    p16(m, 466, 2);
    p16(m, 468, 3);
    p16(m, 470, 1);
    p16(m, 474, 192);
    p16(m, 512, 1);
    p16(m, 514, 62);
    p16(m, 516, 1);
    p16(m, 528, 3);
    for (i, id) in [11, 12, 13].iter().enumerate() {
        p32(m, 544 + i * 16, *id);
        p16(m, 548 + i * 16, 1);
        p32(m, 552 + i * 16, 1);
    }
    p16(m, 592, 1);
    p16(m, 594, 0x8086);
    p16(m, 596, device);
    p16(m, 598, 3);
    p16(m, 604, 65535);
    m[606] = revision.0;
    m[607] = revision.1;
    let g = &mut m[624..816];
    g[..family.len()].copy_from_slice(family.as_bytes());
    for at in [64, 80, 96] {
        p16(g, at, 1);
    }
    for at in [112, 144] {
        p64(g, at, 262144);
        p32(g, at + 16, 16384);
        p16(g, at + 24, 1);
        p16(g, at + 26, 1);
    }
    p64(g, 120, 4096);
    g[176] = 39;
    g[177] = 48;
    g[178] = 1;
    let mut elf = [0; 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    p16(&mut elf, 16, 2);
    p16(&mut elf, 18, 62);
    p32(&mut elf, 20, 1);
    p16(&mut elf, 52, 64);
    for (i, name) in ["gpu-backend", "gpu-renderer"].iter().enumerate() {
        let at = 816 + i * 112;
        p16(m, at, 4 + i as u16);
        p64(m, at + 8, (192 + metadata + i * 64) as u64);
        p64(m, at + 16, 64);
        m[at + 24..at + 56].copy_from_slice(&digest(&elf));
        m[at + 56..at + 56 + name.len()].copy_from_slice(name.as_bytes());
    }
    b[192 + metadata..192 + metadata + 64].copy_from_slice(&elf);
    b[192 + metadata + 64..].copy_from_slice(&elf);
    resign(&mut b);
    b
}
