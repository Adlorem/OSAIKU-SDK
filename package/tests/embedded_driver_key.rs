#[path = "support/gpu_fixture.rs"]
mod fixture;
use fixture::driver::{self as f, p32, p64};
use osaiku_package::{
    self as p,
    application::{stream::Verifier, Manifest},
    driver::{self, installation::Proposal},
    Domain, Error, TrustPolicy,
};

fn keyed(gpu: bool) -> Vec<u8> {
    let mut b = if gpu {
        fixture::package("intel-cometlake", 0x9bc4, (0, 5))
    } else {
        f::package("example.keyed-driver", 1, 1, &[], true)
    };
    p32(&mut b, 192 + 8, 3);
    b[192 + 480..192 + 512].copy_from_slice(&f::key().verifying_key().to_bytes());
    f::resign(&mut b);
    b
}
fn candidate(b: &[u8], old: &TrustPolicy<'_>) -> Result<Proposal, Error> {
    let end = 192 + u32::from_le_bytes(b[204..208].try_into().unwrap()) as usize;
    let m = Manifest::parse_for(&b[192..end], b.len() as u64, Domain::Driver)?;
    let e = p::Envelope::parse(&b[..192], b.len() as u64, Domain::Driver)?;
    Proposal::new(&m, e.key_id(), e.epoch(), old)
}
fn stream(b: &[u8], policy: &TrustPolicy<'_>, chunk: usize) -> Result<[u8; 32], Error> {
    let end = 192 + u32::from_le_bytes(b[204..208].try_into().unwrap()) as usize;
    let mut v = Verifier::new_for(
        &b[..192],
        &b[192..end],
        b.len() as u64,
        policy,
        Domain::Driver,
    )?;
    for part in b[end..].chunks(chunk) {
        v.push(part)?;
    }
    v.finish()
}
#[test]
fn embedded_key_is_signed_but_does_not_grant_runtime_trust() {
    let b = keyed(true);
    let empty = TrustPolicy {
        epoch: 1,
        keys: &[],
    };
    assert!(matches!(
        driver::parse(&b).unwrap().verify(&empty),
        Err(Error::UnknownKey)
    ));
    let proposal = candidate(&b, &empty).unwrap();
    assert!(proposal.requires_consent());
    let policy = proposal.policy();
    assert_eq!(policy.epoch, 2);
    assert_eq!(policy.keys.len(), 2);
    assert_eq!(
        policy.keys.iter().map(|k| k.domain).collect::<Vec<_>>(),
        [Domain::Driver, Domain::DriverGpu]
    );
    assert!(policy
        .keys
        .iter()
        .all(|k| k.minimum_epoch == 2 && !k.revoked));
    driver::parse(&b).unwrap().verify(&policy).unwrap();
    for chunk in [1, 7, 64, 4096] {
        assert_eq!(stream(&b, &policy, chunk).unwrap(), p::digest(&b));
    }
    assert!(driver::parse(&b).unwrap().verify(&empty).is_err());
}
#[test]
fn trusted_updates_do_not_request_or_expand_trust() {
    let b = keyed(true);
    let old = p::policy::PolicyDocument::parse(&f::policy(7, false, &[4, 9])).unwrap();
    let proposal = candidate(&b, &old.policy()).unwrap();
    assert!(!proposal.requires_consent());
    assert_eq!(proposal.policy().epoch, 7);
    assert_eq!(proposal.policy().keys.len(), 2);
    let mut buf = [0; 4096];
    let len = proposal.encode(&mut buf).unwrap();
    assert_eq!(&buf[..len], f::policy(7, false, &[4, 9]));
}
#[test]
fn partial_trust_adds_only_missing_scope_and_preserves_revocation() {
    let b = keyed(true);
    let old = p::policy::PolicyDocument::parse(&f::policy(2, false, &[4])).unwrap();
    let proposal = candidate(&b, &old.policy()).unwrap();
    assert!(proposal.requires_consent());
    assert_eq!(proposal.policy().keys.len(), 2);
    assert_eq!(proposal.policy().epoch, 3);
    for domains in [vec![4], vec![9], vec![4, 9]] {
        let old = p::policy::PolicyDocument::parse(&f::policy(3, true, &domains)).unwrap();
        assert!(matches!(
            candidate(&b, &old.policy()),
            Err(Error::RevokedKey)
        ));
    }
    let mut raised = f::policy(3, false, &[4]);
    p64(&mut raised, 32 + 40, 3);
    let old = p::policy::PolicyDocument::parse(&raised).unwrap();
    assert!(matches!(
        candidate(&b, &old.policy()),
        Err(Error::PolicyEpoch)
    ));
}
#[test]
fn damaged_signature_payload_key_and_reserved_fields_are_refused() {
    let b = keyed(true);
    let old = TrustPolicy {
        epoch: 1,
        keys: &[],
    };
    for at in [128, 160, b.len() - 1] {
        let mut bad = b.clone();
        bad[at] ^= 1;
        let proposal = candidate(&bad, &old).unwrap();
        assert!(stream(&bad, &proposal.policy(), 7).is_err());
    }
    let mut bad = b.clone();
    bad[192 + 480..192 + 512].copy_from_slice(
        &ed25519_dalek::SigningKey::from_bytes(&[0x43; 32])
            .verifying_key()
            .to_bytes(),
    );
    f::resign(&mut bad);
    assert!(matches!(candidate(&bad, &old), Err(Error::Identity)));
    let mut weak = b.clone();
    weak[192 + 480..192 + 512].fill(0);
    f::resign(&mut weak);
    assert!(candidate(&weak, &old).is_err());
    let mut reserved = b.clone();
    reserved[192 + 476] = 1;
    f::resign(&mut reserved);
    assert!(candidate(&reserved, &old).is_err());
}
#[test]
fn legacy_packages_need_existing_trust_and_v3_does_not_change_match_semantics() {
    let old = p::policy::PolicyDocument::parse(&f::policy(2, false, &[4, 5, 9])).unwrap();
    let empty = TrustPolicy {
        epoch: 1,
        keys: &[],
    };
    for gpu in [false, true] {
        let legacy = if gpu {
            fixture::package("intel-cometlake", 0x9bc4, (0, 5))
        } else {
            f::package("example.keyed-driver", 1, 1, &[], true)
        };
        assert!(matches!(candidate(&legacy, &empty), Err(Error::UnknownKey)));
        assert!(!candidate(&legacy, &old.policy())
            .unwrap()
            .requires_consent());
        let b = keyed(gpu);
        let parsed = driver::parse(&b).unwrap();
        let m = parsed.manifest();
        assert_eq!(driver::gpu(m).unwrap().is_some(), gpu);
        if gpu {
            assert!(driver::matches_revision(
                m,
                1,
                [0x8086, 0x9bc4, 3, 0, 0, 0],
                Some(5)
            ));
            assert!(!driver::matches_revision(
                m,
                1,
                [0x8086, 0x9bc4, 3, 0, 0, 0],
                None
            ));
            assert!(!driver::matches_revision(
                m,
                1,
                [0x8086, 0x9bc4, 3, 0, 0, 0],
                Some(6)
            ));
        } else {
            assert!(driver::matches(m, 2, [3; 6]));
            assert_eq!(driver::match_records(m).count(), 1);
        }
    }
}
#[test]
fn key_slots_epochs_and_domains_are_bounded() {
    let b = keyed(true);
    let empty = TrustPolicy {
        epoch: u64::MAX,
        keys: &[],
    };
    assert!(matches!(candidate(&b, &empty), Err(Error::PolicyEpoch)));
    let mut future = b.clone();
    p64(&mut future, 24, u64::MAX);
    f::resign(&mut future);
    assert!(matches!(
        candidate(
            &future,
            &TrustPolicy {
                epoch: 1,
                keys: &[]
            }
        ),
        Err(Error::PolicyEpoch)
    ));
    let foreign = ed25519_dalek::SigningKey::from_bytes(&[0x44; 32])
        .verifying_key()
        .to_bytes();
    let keys = [p::TrustedKey {
        public_key: foreign,
        domain: Domain::Application,
        minimum_epoch: 1,
        revoked: false,
    }; 64];
    assert!(matches!(
        candidate(
            &b,
            &TrustPolicy {
                epoch: 2,
                keys: &keys
            }
        ),
        Err(Error::Limit)
    ));
    let end = 192 + u32::from_le_bytes(b[204..208].try_into().unwrap()) as usize;
    assert!(Manifest::parse_for(&b[192..end], b.len() as u64, Domain::Application).is_err());
}
