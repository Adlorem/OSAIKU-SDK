#[path = "support/driver_fixture.rs"]
mod fixture;
use fixture::*;
use osaiku_package::{digest, policy::PolicyDocument, system::drivers::*, Error};
fn bundle(entries: &[(Vec<u8>, u32, u32)]) -> Vec<u8> {
    let mut out = vec![0u8; HEADER + ROW * entries.len()];
    for (i, (bytes, flags, deps)) in entries.iter().enumerate() {
        while out.len() % 8 != 0 {
            out.push(0)
        }
        let at = HEADER + ROW * i;
        p32(&mut out, at, *flags);
        p32(&mut out, at + 4, *deps);
        let offset = out.len() as u64;
        p64(&mut out, at + 8, offset);
        p64(&mut out, at + 16, bytes.len() as u64);
        out[at + 24..at + 56].copy_from_slice(&digest(bytes));
        out.extend_from_slice(bytes);
    }
    out[..8].copy_from_slice(b"OSADSET1");
    p32(&mut out, 8, 1);
    p32(&mut out, 12, entries.len() as u32);
    let n = out.len() as u64;
    p64(&mut out, 16, n);
    p64(&mut out, 24, 1);
    out
}
fn stream(b: &[u8], domains: &[u8]) -> Result<(), Error> {
    let p = policy(2, false, domains);
    let p = PolicyDocument::parse(&p).unwrap();
    verify_stream(b.len() as u64, &p.policy(), |at, out| {
        let input = b
            .get(at as usize..at as usize + out.len())
            .ok_or(Error::Length)?;
        out.copy_from_slice(input);
        Ok(())
    })
}
#[test]
fn ram_and_stream_agree_on_signed_packages_and_firmware() {
    let bytes = bundle(&[(package("example.boot", 1, 2, &[], true), 3, 0)]);
    let set = Set::parse(&bytes).unwrap();
    let p = policy(2, false, &[4, 6]);
    let p = PolicyDocument::parse(&p).unwrap();
    set.verify(&p.policy()).unwrap();
    stream(&bytes, &[4, 6]).unwrap();
    assert_eq!(set.entries().len(), 1);
    let verified = set.entries().next().unwrap().verified(&p.policy()).unwrap();
    assert_eq!(verified.manifest().components().len(), 2);
}
#[test]
fn dependency_identity_release_digest_order_and_recovery_are_exact() {
    let first = package("example.transport", 2, 8, &[], false);
    let second = package(
        "example.disk",
        1,
        2,
        &[("example.transport", 2, digest(&first))],
        false,
    );
    let valid = bundle(&[(first.clone(), 3, 0), (second.clone(), 3, 1)]);
    Set::parse(&valid).unwrap();
    stream(&valid, &[4, 6]).unwrap();
    for bad in [
        bundle(&[(first.clone(), 0, 0), (second.clone(), 3, 1)]),
        bundle(&[(first.clone(), 3, 0), (second.clone(), 3, 0)]),
        bundle(&[(second, 3, 2), (first, 3, 0)]),
    ] {
        assert!(Set::parse(&bad).is_err());
        assert!(stream(&bad, &[4, 6]).is_err());
    }
}
#[test]
fn firmware_damage_and_missing_tail_refused_before_use() {
    let b = bundle(&[(package("example.boot", 1, 2, &[], true), 3, 0)]);
    for n in [0, 1, 63, 64, 127, b.len() - 1] {
        assert!(Set::parse(&b[..n]).is_err());
        assert!(stream(&b[..n], &[4, 6]).is_err());
    }
    let mut bad = b.clone();
    *bad.last_mut().unwrap() ^= 1;
    let hash = digest(&bad[128..]);
    bad[88..120].copy_from_slice(&hash);
    assert!(stream(&bad, &[4, 6]).is_err());
    let p = policy(2, false, &[4, 6]);
    let p = PolicyDocument::parse(&p).unwrap();
    assert!(Set::parse(&bad)
        .and_then(|s| s.verify(&p.policy()))
        .is_err());
}
#[test]
fn nested_driver_publisher_needs_sensitive_scope() {
    let b = bundle(&[(package("example.boot", 1, 2, &[], false), 3, 0)]);
    assert!(stream(&b, &[3]).is_err());
    assert!(stream(&b, &[4]).is_err());
    stream(&b, &[4, 6]).unwrap();
}
#[test]
fn duplicate_identity_and_forward_edges_refused() {
    let p = package("example.boot", 1, 2, &[], false);
    for b in [
        bundle(&[(p.clone(), 3, 0), (p.clone(), 3, 0)]),
        bundle(&[(p.clone(), 3, 1)]),
        bundle(&[(p, 2, 0)]),
    ] {
        assert!(Set::parse(&b).is_err());
        assert!(stream(&b, &[4, 6]).is_err());
    }
}
#[test]
fn layout_overflow_reserved_and_service_abi_refused() {
    let b = bundle(&[(package("example.boot", 1, 2, &[], false), 3, 0)]);
    for (at, value) in [
        (16, u64::MAX),
        (24, 2),
        (32, 1),
        (72, u64::MAX),
        (80, u64::MAX),
        (120, 1),
    ] {
        let mut bad = b.clone();
        p64(&mut bad, at, value);
        assert!(Set::parse(&bad).is_err(), "at {at}");
        assert!(stream(&bad, &[4, 6]).is_err(), "stream at {at}");
    }
}
#[test]
fn optional_entries_never_gain_recovery_authority() {
    let b = bundle(&[(package("example.optional", 1, 8, &[], false), 0, 0)]);
    let s = Set::parse(&b).unwrap();
    assert_eq!(s.entries().next().unwrap().flags(), 0);
    stream(&b, &[4]).unwrap();
}
#[test]
fn interrupted_source_fails_without_acceptance() {
    let b = bundle(&[(package("example.boot", 1, 2, &[], true), 3, 0)]);
    let p = policy(2, false, &[4, 6]);
    let p = PolicyDocument::parse(&p).unwrap();
    let mut reads = 0;
    assert!(verify_stream(b.len() as u64, &p.policy(), |at, out| {
        reads += 1;
        if reads == 5 {
            return Err(Error::Failed);
        }
        out.copy_from_slice(&b[at as usize..at as usize + out.len()]);
        Ok(())
    })
    .is_err());
}

#[test]
fn correctly_signed_new_process_abi_cannot_enter_an_old_boot_set() {
    let mut driver = package("example.future", 1, 2, &[], false);
    p16(&mut driver, 192 + 516, 2);
    resign(&mut driver);
    let b = bundle(&[(driver, 3, 0)]);
    assert!(matches!(Set::parse(&b), Err(Error::Compatibility)));
    assert_eq!(stream(&b, &[4, 6]), Err(Error::Compatibility));
}
