//! Bounded beacon/probe-response decoding. Discovery is untrusted metadata,
//! never authentication, regulatory permission or proof that a BSS is reachable.
use crate::{error, security, Bss, Ssid};
fn word(b: &[u8], at: usize) -> Result<u16, i64> {
    Ok(u16::from_le_bytes(
        b.get(at..at + 2).ok_or(error::INVALID)?.try_into().unwrap(),
    ))
}
fn rsn(b: &[u8]) -> Result<u8, i64> {
    if word(b, 0)? != 1 {
        return Err(error::INVALID);
    }
    let Some(group) = b.get(2..6) else {
        return Err(error::INVALID);
    };
    let mut at = 6;
    let pairs = word(b, at)? as usize;
    at += 2;
    if pairs == 0 || pairs > 16 {
        return Err(error::INVALID);
    }
    let mut ccmp = false;
    for _ in 0..pairs {
        let suite = b.get(at..at + 4).ok_or(error::INVALID)?;
        at += 4;
        ccmp |= suite == [0, 15, 172, 4];
    }
    let akms = word(b, at)? as usize;
    at += 2;
    if akms == 0 || akms > 16 {
        return Err(error::INVALID);
    }
    let mut psk = false;
    let mut sae = false;
    let mut enterprise = false;
    for _ in 0..akms {
        let suite = b.get(at..at + 4).ok_or(error::INVALID)?;
        at += 4;
        psk |= suite == [0, 15, 172, 2];
        sae |= suite == [0, 15, 172, 8];
        enterprise |= suite == [0, 15, 172, 1];
    }
    let caps = if at == b.len() {
        0
    } else {
        let caps = word(b, at)?;
        at += 2;
        caps
    };
    if at < b.len() {
        let count = word(b, at)? as usize;
        at += 2;
        if count > 16 {
            return Err(error::INVALID);
        }
        at = at.checked_add(count * 16).ok_or(error::INVALID)?;
        if at > b.len() {
            return Err(error::INVALID);
        }
        if at < b.len() {
            if b.len() - at != 4 {
                return Err(error::INVALID);
            }
            at += 4;
        }
    }
    if at != b.len() {
        return Err(error::INVALID);
    }
    // No TKIP/WEP fallback, no claim of PMF/SAE support in the initial profile.
    if psk && ccmp && group == [0, 15, 172, 4] && caps & (1 << 6) == 0 {
        Ok(security::WPA2_PSK_CCMP)
    } else if sae {
        Ok(security::WPA3_SAE)
    } else if enterprise {
        Ok(security::ENTERPRISE)
    } else {
        Ok(security::UNSUPPORTED)
    }
}
pub fn beacon(
    frame: &[u8],
    frequency_mhz: u16,
    signal_dbm: i16,
    now: u64,
    generation: u64,
) -> Result<Bss, i64> {
    if frame.len() < 36
        || frame.len() > 4096
        || generation == 0
        || !(-127..=0).contains(&signal_dbm)
    {
        return Err(error::INVALID);
    }
    let fc = word(frame, 0)?;
    if fc & 0x00ff != 0x80 && fc & 0x00ff != 0x50 || fc & 0xc700 != 0 || word(frame, 22)? & 15 != 0
    {
        return Err(error::INVALID);
    }
    let privacy = word(frame, 34)? & (1 << 4) != 0;
    let mut bssid = [0; 6];
    bssid.copy_from_slice(&frame[16..22]);
    if bssid == [0; 6] || bssid[0] & 1 != 0 || frame[10..16] != bssid {
        return Err(error::INVALID);
    }
    if !(2412..=2484).contains(&frequency_mhz) && !(4910..=5905).contains(&frequency_mhz) {
        return Err(error::UNSUPPORTED);
    }
    let mut ssid = None;
    let mut protection = None;
    let mut at = 36;
    while at < frame.len() {
        let header = frame.get(at..at + 2).ok_or(error::INVALID)?;
        let kind = header[0];
        let len = header[1] as usize;
        at += 2;
        let data = frame.get(at..at + len).ok_or(error::INVALID)?;
        at += len;
        match kind {
            0 => {
                if ssid.is_some() {
                    return Err(error::INVALID);
                }
                ssid = Some(Ssid::new(data)?);
            }
            48 => {
                if protection.is_some() {
                    return Err(error::INVALID);
                }
                protection = Some(rsn(data)?);
            }
            _ => (),
        }
    }
    if protection.is_some() && !privacy {
        return Err(error::INVALID);
    }
    Ok(Bss {
        ssid: ssid.ok_or(error::INVALID)?,
        bssid,
        frequency_mhz,
        signal_dbm,
        security: protection.unwrap_or(if privacy {
            security::UNSUPPORTED
        } else {
            security::OPEN
        }),
        seen_ms: now,
        generation,
    })
}
