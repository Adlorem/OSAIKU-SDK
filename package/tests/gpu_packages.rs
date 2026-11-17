#[path = "support/gpu_fixture.rs"]
mod fixture;
use fixture::{driver::*, *};
use osaiku_package::{self as p, driver, policy::PolicyDocument, Domain};

#[test]
fn two_executables_are_authenticated_in_both_verifiers() {
    let bytes = fixture::package("intel-cometlake", 0x9bc4, (0, 5));
    let policy = PolicyDocument::parse(&policy(2, false, &[4, 9])).unwrap();
    let parsed = driver::parse(&bytes).unwrap();
    let roles: Vec<_> = parsed
        .manifest()
        .components()
        .map(|c| (c.kind(), c.name()))
        .collect();
    assert_eq!(roles, [(4, "gpu-backend"), (5, "gpu-renderer")]);
    assert_eq!(
        parsed.verify(&policy.policy()).unwrap().sha256(),
        p::digest(&bytes)
    );
    let end = TABLE + 224;
    for size in [1, 7, 63, 64, 127] {
        let mut stream = p::application::stream::Verifier::new_for(
            &bytes[..192],
            &bytes[192..end],
            bytes.len() as u64,
            &policy.policy(),
            Domain::Driver,
        )
        .unwrap();
        for part in bytes[end..].chunks(size) {
            stream.push(part).unwrap();
        }
        assert_eq!(stream.finish().unwrap(), p::digest(&bytes));
    }
}

#[test]
fn raw_gpu_scope_is_separate_from_driver_and_controller_trust() {
    let bytes = fixture::package("intel-cometlake", 0x9bc4, (0, 5));
    for domains in [&[4][..], &[4, 7][..], &[9][..]] {
        let policy = PolicyDocument::parse(&policy(2, false, domains)).unwrap();
        assert!(driver::parse(&bytes)
            .unwrap()
            .verify(&policy.policy())
            .is_err());
    }
    let policy = PolicyDocument::parse(&policy(2, true, &[4, 9])).unwrap();
    assert!(driver::parse(&bytes)
        .unwrap()
        .verify(&policy.policy())
        .is_err());
    let m = driver::parse(&bytes).unwrap();
    driver::TrustClass::Installable.admit(m.manifest()).unwrap();
    // GPU support must not weaken the existing controller restriction.
    let controller = fixture::driver::package("controller", 1, 4, &[], false);
    assert!(driver::TrustClass::Installable
        .admit(driver::parse(&controller).unwrap().manifest())
        .is_err());
}

#[test]
fn exact_device_revision_and_distinct_families_are_required() {
    let cml = fixture::package("intel-cometlake", 0x9bc4, (0, 5));
    let tgl = fixture::package("intel-tigerlake", 0x9a49, (1, 2));
    let identity = [0x8086, 0x9bc4, 3, 0, 0, 5];
    let c = driver::parse(&cml).unwrap();
    let t = driver::parse(&tgl).unwrap();
    assert!(!driver::matches(c.manifest(), 1, identity));
    assert_eq!(driver::match_records(c.manifest()).count(), 0);
    for revision in 0..=5 {
        assert!(driver::matches_revision(
            c.manifest(),
            1,
            identity,
            Some(revision)
        ));
    }
    assert!(!driver::matches_revision(
        c.manifest(),
        1,
        identity,
        Some(6)
    ));
    assert!(!driver::matches_revision(
        t.manifest(),
        1,
        identity,
        Some(1)
    ));
    assert!(!driver::matches_revision(
        c.manifest(),
        2,
        identity,
        Some(1)
    ));
    assert_ne!(
        driver::gpu(c.manifest()).unwrap().unwrap().family,
        driver::gpu(t.manifest()).unwrap().unwrap().family
    );
}

#[test]
fn malformed_roles_revisions_budgets_and_optional_protocols_are_refused() {
    let bytes = fixture::package("intel-cometlake", 0x9bc4, (0, 5));
    for (at, value) in [
        (TABLE, 1),
        (TABLE + 112, 2),
        (MATCH + 14, 6),
        (GPU + 176, 31),
        (GPU + 178, 0),
        (GPU + 179, 1),
        (GPU + 144 + 8, 1),
        (GPU + 112 + 26, 2),
        (GPU + 64, 0),
        (192 + 544 + 8, 0),
        (192 + 448, 4),
        (192 + 474, 0),
    ] {
        let mut bad = bytes.clone();
        bad[at] = value;
        resign(&mut bad);
        assert!(driver::parse(&bad).is_err(), "offset {at}");
    }
    for end in 0..bytes.len() {
        assert!(driver::parse(&bytes[..end]).is_err());
    }
}

#[test]
fn renderer_is_an_elf_not_firmware_even_after_valid_resigning() {
    let mut b = fixture::package("intel-cometlake", 0x9bc4, (0, 5));
    let start = TABLE + 224 + 64;
    b[start] = 0;
    let hash = p::digest(&b[start..]);
    b[TABLE + 112 + 24..TABLE + 112 + 56].copy_from_slice(&hash);
    resign(&mut b);
    assert!(matches!(driver::parse(&b), Err(p::Error::Elf)));
    let policy = PolicyDocument::parse(&policy(2, false, &[4, 9])).unwrap();
    let end = TABLE + 224;
    let mut v = p::application::stream::Verifier::new_for(
        &b[..192],
        &b[192..end],
        b.len() as u64,
        &policy.policy(),
        Domain::Driver,
    )
    .unwrap();
    assert_eq!(v.push(&b[end..]), Err(p::Error::Elf));
    assert!(v.finish().is_err());
}

#[test]
fn three_interface_versions_are_checked_independently_without_kernel_identity() {
    use p::compatibility::{Platform, ProtocolSupport, Version};
    let b = fixture::package("intel-cometlake", 0x9bc4, (0, 5));
    let p = driver::parse(&b).unwrap();
    let required = p.manifest().compatibility();
    let good = [11, 12, 13].map(|id| ProtocolSupport {
        features: 0,
        id,
        version: Version { major: 1, minor: 9 },
    });
    required
        .check(&Platform {
            profile: required.profile(),
            protocols: &good,
        })
        .unwrap();
    for i in 0..3 {
        let mut wrong = good;
        wrong[i].version.major = 2;
        assert!(required
            .check(&Platform {
                profile: required.profile(),
                protocols: &wrong
            })
            .is_err());
    }
}

#[test]
fn device_local_descriptor_does_not_claim_uma_compatibility() {
    use driver::gpu_protocol::MemoryModel;
    let mut b = fixture::package("contrast-device-local", 0x1234, (0, 255));
    b[GPU + 178] = 2;
    resign(&mut b);
    let p = driver::parse(&b).unwrap();
    assert_eq!(
        driver::gpu(p.manifest()).unwrap().unwrap().memory,
        MemoryModel::DeviceLocal
    );
}

#[test]
fn signed_feature_requirements_cannot_be_satisfied_by_a_version_or_another_interface() {
    use p::compatibility::{CompatibilityError, Platform, ProtocolSupport, Version};
    for index in 0..3 {
        let mut b = fixture::package("intel-cometlake", 0x9bc4, (0, 5));
        // Exercise all 64 bits, including a future feature unknown to this SDK.
        for feature in [1u64, 1 << 63] {
            b[GPU + 72 + index * 16..GPU + 80 + index * 16].copy_from_slice(&feature.to_le_bytes());
            resign(&mut b);
            let parsed = driver::parse(&b).unwrap();
            let requirements = parsed.manifest().compatibility();
            let mut protocols = [11, 12, 13].map(|id| ProtocolSupport {
                id,
                version: Version { major: 1, minor: 9 },
                features: u64::MAX,
            });
            protocols[index].features = !feature;
            assert_eq!(
                requirements.check(&Platform {
                    profile: requirements.profile(),
                    protocols: &protocols,
                }),
                Err(CompatibilityError::RequiredFeatures(11 + index as u32))
            );
            protocols[index].features = feature;
            requirements
                .check(&Platform {
                    profile: requirements.profile(),
                    protocols: &protocols,
                })
                .unwrap();
        }
    }
}
