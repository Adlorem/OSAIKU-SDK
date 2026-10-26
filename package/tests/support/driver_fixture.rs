use ed25519_dalek::{Signer, SigningKey};
use osaiku_package::{digest, Domain};
pub fn p16(b: &mut [u8], i: usize, v: u16) {
    b[i..i + 2].copy_from_slice(&v.to_le_bytes());
}
pub fn p32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}
pub fn p64(b: &mut [u8], i: usize, v: u64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}
pub fn key() -> SigningKey {
    SigningKey::from_bytes(&[0x42; 32])
}
pub fn publisher() -> [u8; 32] {
    digest(&key().verifying_key().to_bytes())
}
pub fn resign(b: &mut [u8]) {
    let len = b.len() as u64;
    p64(b, 16, len);
    p64(b, 96, len - 192);
    let hash = digest(&b[192..]);
    b[64..96].copy_from_slice(&hash);
    let mut message = Domain::Driver.signature_domain().to_vec();
    message.extend_from_slice(&b[..128]);
    b[128..192].copy_from_slice(&key().sign(&message).to_bytes());
}
pub fn package(
    id: &str,
    release: u64,
    classes: u64,
    deps: &[(&str, u64, [u8; 32])],
    firmware: bool,
) -> Vec<u8> {
    let table = 512 + 32 + 32 + 208 * deps.len();
    let metadata = table + 112 * (if firmware { 2 } else { 1 });
    let mut b = vec![0; 192 + metadata + 64 + if firmware { 8 } else { 0 }];
    b[..8].copy_from_slice(Domain::Driver.magic());
    p32(&mut b, 8, 1);
    p32(&mut b, 12, 192);
    p64(&mut b, 24, 2);
    b[32..64].copy_from_slice(&publisher());
    let m = &mut b[192..192 + metadata];
    m[..8].copy_from_slice(b"OSADMF01");
    p32(m, 8, 1);
    p32(m, 12, metadata as u32);
    p64(m, 16, release);
    p64(m, 32, 262144);
    p64(m, 40, 65536);
    p16(m, 48, 1);
    p16(m, 50, 1);
    p16(m, 54, 1);
    p16(m, 60, 32);
    p16(m, 62, if firmware { 2 } else { 1 });
    m[64..64 + id.len()].copy_from_slice(id.as_bytes());
    m[192..198].copy_from_slice(b"Driver");
    m[320] = b'1';
    m[384] = b'1';
    p64(m, 448, classes);
    p32(m, 456, 50);
    p16(m, 464, 8);
    p16(m, 466, 1);
    p16(m, 468, 1);
    p16(m, 470, 1);
    p16(m, 472, deps.len() as u16);
    p16(m, 512, 1);
    p16(m, 514, 62);
    p16(m, 516, 1);
    p16(m, 544, 2);
    for i in 1..7 {
        p16(m, 544 + i * 2, 65535);
    }
    for (i, (id, release, hash)) in deps.iter().enumerate() {
        let at = 576 + i * 208;
        m[at..at + 32].copy_from_slice(&publisher());
        m[at + 32..at + 32 + id.len()].copy_from_slice(id.as_bytes());
        p64(m, at + 160, *release);
        m[at + 168..at + 200].copy_from_slice(hash);
    }
    let mut elf = [0; 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    p16(&mut elf, 16, 2);
    p16(&mut elf, 18, 62);
    p32(&mut elf, 20, 1);
    p16(&mut elf, 52, 64);
    p16(m, table, 1);
    p64(m, table + 8, (192 + metadata) as u64);
    p64(m, table + 16, 64);
    m[table + 24..table + 56].copy_from_slice(&digest(&elf));
    m[table + 56..table + 66].copy_from_slice(b"executable");
    if firmware {
        let at = table + 112;
        p16(m, at, 2);
        p64(m, at + 8, (192 + metadata + 64) as u64);
        p64(m, at + 16, 8);
        m[at + 24..at + 56].copy_from_slice(&digest(b"firmware"));
        m[at + 56..at + 63].copy_from_slice(b"fw-main");
    }
    b[192 + metadata..192 + metadata + 64].copy_from_slice(&elf);
    if firmware {
        b[192 + metadata + 64..].copy_from_slice(b"firmware");
    }
    resign(&mut b);
    b
}
pub fn policy(epoch: u64, revoked: bool, domains: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 32 + 64 * domains.len()];
    b[..8].copy_from_slice(b"OSAPOL01");
    p32(&mut b, 8, 1);
    p32(&mut b, 12, domains.len() as u32);
    p64(&mut b, 16, epoch);
    for (i, domain) in domains.iter().enumerate() {
        let at = 32 + i * 64;
        b[at..at + 32].copy_from_slice(&key().verifying_key().to_bytes());
        b[at + 32] = *domain;
        b[at + 33] = u8::from(revoked);
        p64(&mut b, at + 40, 2);
    }
    b
}
