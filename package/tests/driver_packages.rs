#[path = "support/driver_fixture.rs"]
mod fixture;
use fixture::*;
use osaiku_package::{self as p, driver, policy::PolicyDocument, Domain};
#[test]
fn driver_domain_and_sensitive_publisher_scopes_are_independent() {
    let b = package("example.input", 1, 1, &[], false);
    for domains in [&[1, 2, 3][..], &[4][..], &[5][..]] {
        let policy = policy(2, false, domains);
        assert!(driver::parse(&b)
            .unwrap()
            .verify(&PolicyDocument::parse(&policy).unwrap().policy())
            .is_err());
    }
    let policy = policy(2, false, &[4, 5]);
    driver::parse(&b)
        .unwrap()
        .verify(&PolicyDocument::parse(&policy).unwrap().policy())
        .unwrap();
    assert!(p::application::Package::parse(&b).is_err());
    let mut forged = b.clone();
    forged[..8].copy_from_slice(Domain::Application.magic());
    assert!(p::application::Package::parse(&forged).is_err());
}
#[test]
fn streaming_and_contiguous_driver_verification_agree() {
    let b = package("example.status", 1, 8, &[], true);
    let policy = policy(2, false, &[4]);
    let policy = PolicyDocument::parse(&policy).unwrap();
    let end = 192 + u32::from_le_bytes(b[204..208].try_into().unwrap()) as usize;
    for chunk in [1, 7, 64, 4096] {
        let mut v = p::application::stream::Verifier::new_for(
            &b[..192],
            &b[192..end],
            b.len() as u64,
            &policy.policy(),
            Domain::Driver,
        )
        .unwrap();
        for c in b[end..].chunks(chunk) {
            v.push(c).unwrap();
        }
        assert_eq!(v.finish().unwrap(), p::digest(&b));
    }
    assert_eq!(
        driver::parse(&b)
            .unwrap()
            .verify(&policy.policy())
            .unwrap()
            .sha256(),
        p::digest(&b)
    );
}
#[test]
fn driver_manifest_rejects_reserved_ranges_limits_windows_and_unknown_versions() {
    let b = package("example.status", 1, 8, &[], true);
    for (at, value) in [
        (200, 2),
        (192 + 474, 1),
        (192 + 466, 9),
        (192 + 468, 0),
        (192 + 470, 0),
        (192 + 54, 2),
        (192 + 464, 0),
        (192 + 456, 0),
        (192 + 448, 0),
    ] {
        let mut bad = b.clone();
        bad[at] = value;
        resign(&mut bad);
        assert!(driver::parse(&bad).is_err(), "{at}");
    }
    for end in 0..b.len() {
        assert!(driver::parse(&b[..end]).is_err());
    }
    let mut huge = b.clone();
    p32(&mut huge, 204, u32::MAX);
    assert!(driver::parse(&huge).is_err());
}
#[test]
fn firmware_and_dependencies_are_named_bounded_exact_and_authenticated() {
    let provider = package("example.provider", 2, 8, &[], false);
    let b = package(
        "example.client",
        1,
        8,
        &[("example.provider", 2, p::digest(&provider))],
        true,
    );
    let parsed = driver::parse(&b).unwrap();
    let m = parsed.manifest();
    let dep = driver::dependencies(m).next().unwrap();
    assert_eq!(dep.digest, p::digest(&provider));
    assert_eq!(dep.release, 2);
    assert!(driver::matches(m, 2, [1, 2, 3, 4, 5, 6]));
    assert!(!driver::matches(m, 1, [1, 2, 3, 4, 5, 6]));
    let mut bad = b.clone();
    let len = bad.len();
    bad[len - 1] ^= 1;
    assert!(driver::parse(&bad).is_err());
    let mut bad = b.clone();
    bad[192 + 576 + 32] = b'/';
    resign(&mut bad);
    assert!(driver::parse(&bad).is_err());
}
#[test]
fn revocation_epoch_and_component_mismatch_never_verify() {
    let b = package("example.input", 1, 7, &[], false);
    for (epoch, revoked, domains) in [
        (3, true, vec![4, 5, 6, 7]),
        (1, false, vec![4, 5, 6, 7]),
        (2, false, vec![4, 5, 6]),
    ] {
        let bytes = policy(epoch, revoked, &domains);
        let result = PolicyDocument::parse(&bytes)
            .and_then(|policy| driver::parse(&b)?.verify(&policy.policy()).map(|_| ()));
        assert!(result.is_err());
    }
}

#[test]
fn dependency_graph_rejects_cycles_and_out_of_set_edges() {
    assert!(driver::acyclic(&[2, 4, 0]));
    assert!(!driver::acyclic(&[2, 4, 1]));
    assert!(!driver::acyclic(&[1]));
    assert!(!driver::acyclic(&[8, 0]));
}
