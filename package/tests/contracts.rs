use ed25519_dalek::{Signer, SigningKey};
use osaiku_package::{application::*, policy::PolicyDocument, repository::*, system, *};

// Public deterministic TEST-ONLY key. Never installed in a runtime trust policy.
fn key() -> SigningKey {
    SigningKey::from_bytes(&[0x42; 32])
}
fn trust(domain: Domain) -> TrustedKey {
    TrustedKey {
        public_key: key().verifying_key().to_bytes(),
        domain,
        minimum_epoch: 2,
        revoked: false,
    }
}
fn put16(b: &mut [u8], o: usize, n: u16) {
    b[o..o + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(b: &mut [u8], o: usize, n: u32) {
    b[o..o + 4].copy_from_slice(&n.to_le_bytes());
}
fn put64(b: &mut [u8], o: usize, n: u64) {
    b[o..o + 8].copy_from_slice(&n.to_le_bytes());
}
fn string(b: &mut [u8], o: usize, s: &str) {
    b[o..o + s.len()].copy_from_slice(s.as_bytes());
}
fn signed(domain: Domain, payload: &[u8], epoch: u64) -> Vec<u8> {
    let mut out = vec![0; 192];
    out[..8].copy_from_slice(domain.magic());
    put32(&mut out, 8, 1);
    put32(&mut out, 12, 192);
    put64(&mut out, 16, (192 + payload.len()) as u64);
    put64(&mut out, 24, epoch);
    out[32..64].copy_from_slice(&digest(&key().verifying_key().to_bytes()));
    out[64..96].copy_from_slice(&digest(payload));
    put64(&mut out, 96, payload.len() as u64);
    let mut message = domain.signature_domain().to_vec();
    message.extend_from_slice(&out[..128]);
    out[128..192].copy_from_slice(&key().sign(&message).to_bytes());
    out.extend_from_slice(payload);
    out
}
fn app() -> Vec<u8> {
    let mut p = vec![0; 656];
    p[..8].copy_from_slice(b"OSAMNF01");
    put32(&mut p, 8, 1);
    put32(&mut p, 12, 656);
    put64(&mut p, 16, 5);
    put64(&mut p, 32, 1024 * 1024);
    put64(&mut p, 40, 65536);
    put16(&mut p, 48, 1);
    put16(&mut p, 50, 1);
    put16(&mut p, 54, 1);
    put16(&mut p, 60, 32);
    put16(&mut p, 62, 1);
    string(&mut p, 64, "dev.example.hello");
    string(&mut p, 192, "Hello");
    string(&mut p, 320, "1.0");
    string(&mut p, 384, "0.1.0");
    put16(&mut p, 512, 1);
    put16(&mut p, 514, 62);
    put16(&mut p, 516, 1);
    let mut elf = vec![0; 64];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    put16(&mut elf, 16, 2);
    put16(&mut elf, 18, 62);
    put32(&mut elf, 20, 1);
    put16(&mut elf, 52, 64);
    put16(&mut p, 544, 1);
    put64(&mut p, 552, 848);
    put64(&mut p, 560, 64);
    p[568..600].copy_from_slice(&digest(&elf));
    string(&mut p, 600, "executable");
    p.extend(elf);
    signed(Domain::Application, &p, 2)
}
fn index(app: &[u8]) -> Vec<u8> {
    let mut p = vec![0; 160 + 512];
    string(&mut p, 0, "dev.example.repository");
    put64(&mut p, 128, 3);
    put64(&mut p, 136, 100);
    put64(&mut p, 144, 1000);
    put32(&mut p, 152, 1);
    p[160..192].copy_from_slice(&app[32..64]);
    p[192..320].copy_from_slice(&app[256..384]);
    put64(&mut p, 320, 5);
    put64(&mut p, 328, app.len() as u64);
    p[336..368].copy_from_slice(&digest(app));
    put16(&mut p, 368, 62);
    put16(&mut p, 370, 1);
    string(&mut p, 376, "https://packages.example/hello.osapp");
    signed(Domain::Repository, &p, 2)
}
fn policy<'a>(keys: &'a [TrustedKey]) -> TrustPolicy<'a> {
    TrustPolicy { epoch: 2, keys }
}

#[test]
fn exact_signed_bytes_and_publisher_are_verified_without_granting_a_role() {
    let data = app();
    let keys = [trust(Domain::Application)];
    let p = Package::parse(&data)
        .unwrap()
        .verify(&policy(&keys))
        .unwrap();
    assert_eq!(p.manifest().id(), "dev.example.hello");
    assert_eq!(p.sha256(), digest(&data));
    assert_eq!(p.manifest().permissions(), 0);
    assert_eq!(p.check_rollback(6, None), Err(Error::Rollback));
    assert_eq!(p.check_rollback(6, Some([0; 32])), Err(Error::Rollback));
    assert!(p.check_rollback(6, Some(p.sha256())).is_ok());
}
#[test]
fn unknown_revoked_wrong_domain_duplicate_and_outdated_policies_fail_closed() {
    let data = app();
    for (keys, epoch, expected) in [
        (vec![], 2, Error::UnknownKey),
        (vec![trust(Domain::Repository)], 2, Error::UnknownKey),
        (
            vec![TrustedKey {
                revoked: true,
                ..trust(Domain::Application)
            }],
            2,
            Error::RevokedKey,
        ),
        (
            vec![trust(Domain::Application), trust(Domain::Application)],
            2,
            Error::Policy,
        ),
        (
            vec![TrustedKey {
                minimum_epoch: 3,
                ..trust(Domain::Application)
            }],
            3,
            Error::PolicyEpoch,
        ),
        (
            vec![TrustedKey {
                minimum_epoch: 1,
                ..trust(Domain::Application)
            }],
            1,
            Error::PolicyEpoch,
        ),
    ] {
        assert_eq!(
            Package::parse(&data)
                .unwrap()
                .verify(&TrustPolicy { epoch, keys: &keys })
                .unwrap_err(),
            expected
        );
    }
}
#[test]
fn signed_metadata_payload_and_domain_cannot_be_substituted() {
    let data = app();
    let keys = [trust(Domain::Application)];
    for at in [24, 32, 64, 128, 191, 208, 384, 848, 900] {
        let mut bad = data.clone();
        bad[at] ^= 1;
        assert!(
            Package::parse(&bad)
                .and_then(|p| p.verify(&policy(&keys)))
                .is_err(),
            "byte {at}"
        );
    }
    let bad = signed(Domain::Repository, &data[192..], 2);
    assert!(Package::parse(&bad).is_err());
}
#[test]
fn every_truncation_and_trailing_byte_is_refused() {
    let data = app();
    for length in 0..data.len() {
        assert!(Package::parse(&data[..length]).is_err(), "length {length}");
    }
    let mut bad = data;
    bad.push(0);
    assert!(Package::parse(&bad).is_err());
}
#[test]
fn manifest_ranges_reserved_id_privileges_and_budgets_are_checked() {
    let data = app();
    for (offset, value) in [
        (16, 0),
        (24, 1 << 63),
        (32, 0),
        (32, u64::MAX),
        (40, u64::MAX),
        (552, u64::MAX),
        (560, u64::MAX),
    ] {
        let mut p = data[192..].to_vec();
        put64(&mut p, offset, value);
        assert!(Package::parse(&signed(Domain::Application, &p, 2)).is_err());
    }
    for offset in [448, 511, 546, 550] {
        let mut p = data[192..].to_vec();
        p[offset] = 1;
        assert!(Package::parse(&signed(Domain::Application, &p, 2)).is_err());
    }
    for id in [
        "org.osaiku.themes",
        "initd",
        "osaiku.foo",
        "../x",
        "dev..hello",
        "Dev.hello",
    ] {
        let mut p = data[192..].to_vec();
        p[64..192].fill(0);
        string(&mut p, 64, id);
        assert!(
            Package::parse(&signed(Domain::Application, &p, 2)).is_err(),
            "{id}"
        );
    }
    for (offset, value) in [
        (48, 0),
        (48, 17),
        (50, 65),
        (52, 17),
        (54, 3),
        (60, 545),
        (62, 130),
    ] {
        let mut p = data[192..].to_vec();
        put16(&mut p, offset, value);
        assert!(Package::parse(&signed(Domain::Application, &p, 2)).is_err());
    }
}
#[test]
fn streaming_authentication_is_bounded_terminal_and_checks_exact_length() {
    let data = app();
    let keys = [trust(Domain::Application)];
    let policy = policy(&keys);
    let envelope = Envelope::parse(&data[..192], data.len() as u64, Domain::Application).unwrap();
    let mut stream = envelope.begin(&policy).unwrap();
    for chunk in data[192..].chunks(7) {
        stream.push(chunk).unwrap();
    }
    assert_eq!(stream.finish().unwrap(), digest(&data));
    assert_eq!(
        envelope.begin(&policy).unwrap().finish(),
        Err(Error::Incomplete)
    );
    for chunk in [vec![], vec![0; CHUNK_BYTES + 1], vec![0; data.len()]] {
        let mut stream = envelope.begin(&policy).unwrap();
        assert_eq!(stream.push(&chunk), Err(Error::Chunk));
        assert_eq!(stream.push(&[0]), Err(Error::Failed));
        assert_eq!(stream.finish(), Err(Error::Failed));
    }
}
#[test]
fn repository_time_checkpoint_identity_and_package_binding_are_independent() {
    let data = app();
    let repo = index(&data);
    let keys = [trust(Domain::Application), trust(Domain::Repository)];
    let policy = policy(&keys);
    let index = Index::parse(&repo).unwrap();
    let initial = RepositoryState {
        id: "dev.example.repository",
        sequence: 0,
        digest: [0; 32],
    };
    let hash = index.verify(&policy, Some(500), &initial).unwrap();
    for now in [None, Some(99), Some(1000)] {
        assert_eq!(index.verify(&policy, now, &initial), Err(Error::Time));
    }
    assert_eq!(
        index.verify(
            &policy,
            Some(500),
            &RepositoryState {
                id: initial.id,
                sequence: 4,
                digest: hash
            }
        ),
        Err(Error::Rollback)
    );
    assert_eq!(
        index.verify(
            &policy,
            Some(500),
            &RepositoryState {
                id: initial.id,
                sequence: 3,
                digest: [0; 32]
            }
        ),
        Err(Error::Equivocation)
    );
    assert_eq!(
        index.verify(
            &policy,
            Some(500),
            &RepositoryState {
                id: "different",
                sequence: 0,
                digest: [0; 32]
            }
        ),
        Err(Error::Repository)
    );
    assert!(index
        .verify(
            &policy,
            Some(500),
            &RepositoryState {
                id: initial.id,
                sequence: 3,
                digest: hash
            }
        )
        .is_ok());
    let package = Package::parse(&data).unwrap().verify(&policy).unwrap();
    index.entries().next().unwrap().bind(&package).unwrap();
    let mut changed = data[192..].to_vec();
    put64(&mut changed, 16, 6);
    let changed = signed(Domain::Application, &changed, 2);
    let package = Package::parse(&changed).unwrap().verify(&policy).unwrap();
    assert_eq!(
        index.entries().next().unwrap().bind(&package),
        Err(Error::Binding)
    );
}
#[test]
fn repository_cannot_smuggle_credentials_redirect_targets_or_duplicate_entries() {
    for url in [
        "http://packages.example/a",
        "https://user@packages.example/a",
        "https://127.0.0.1/a",
        "https://host:443/a",
        "https://host/a?x=1",
        "https://host/a#x",
        "https://host/%2e%2e/a",
        "https://host/../a",
        "https://host//a",
        "https://host/",
        "https://HOST/a",
    ] {
        assert!(!https_url(url), "{url}");
    }
    assert!(https_url("https://packages.example/a/b-1.osapp"));
    let repo = index(&app());
    let mut payload = repo[192..].to_vec();
    payload.extend_from_slice(&repo[352..]);
    put32(&mut payload, 152, 2);
    assert!(Index::parse(&signed(Domain::Repository, &payload, 2)).is_err());
    for (offset, value) in [
        (136, 0),
        (144, 100),
        (144, 100 + MAX_VALIDITY_SECONDS + 1),
        (128, 0),
    ] {
        let mut p = repo[192..].to_vec();
        put64(&mut p, offset, value);
        assert!(Index::parse(&signed(Domain::Repository, &p, 2)).is_err());
    }
}
#[test]
fn public_policy_record_cannot_reinterpret_status_domain_or_duplicate_keys() {
    let mut bytes = vec![0; 96];
    bytes[..8].copy_from_slice(b"OSAPOL01");
    put32(&mut bytes, 8, 1);
    put32(&mut bytes, 12, 1);
    put64(&mut bytes, 16, 2);
    bytes[32..64].copy_from_slice(&key().verifying_key().to_bytes());
    bytes[64] = 1;
    put64(&mut bytes, 72, 2);
    let policy = PolicyDocument::parse(&bytes).unwrap();
    Package::parse(&app())
        .unwrap()
        .verify(&policy.policy())
        .unwrap();
    for offset in [24, 31, 66, 71, 80, 95] {
        let mut bad = bytes.clone();
        bad[offset] = 1;
        assert!(PolicyDocument::parse(&bad).is_err());
    }
    for (offset, value) in [(64, 0), (64, 4), (65, 2)] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        assert!(PolicyDocument::parse(&bad).is_err());
    }
    let record = bytes[32..].to_vec();
    bytes.extend(record);
    put32(&mut bytes, 12, 2);
    assert!(PolicyDocument::parse(&bytes).is_err());
}
#[test]
fn system_release_pair_uses_separate_trust_and_rejects_old_handoff() {
    let mut p = vec![0; 128];
    put64(&mut p, 0, 1);
    put32(&mut p, 8, 7);
    put32(&mut p, 12, 1);
    put64(&mut p, 16, 1);
    put64(&mut p, 24, 4096);
    p[32..64].fill(1);
    put64(&mut p, 64, 8192);
    p[72..104].fill(2);
    let bytes = signed(Domain::System, &p, 2);
    let r = system::Release::parse(&bytes).unwrap();
    let keys = [trust(Domain::System)];
    assert!(r.verify(&policy(&keys)).is_ok());
    assert_eq!(
        r.verify(&policy(&[trust(Domain::Application)])),
        Err(Error::UnknownKey)
    );
    put32(&mut p, 8, 6);
    assert!(system::Release::parse(&signed(Domain::System, &p, 2)).is_err());
}
#[test]
fn arbitrary_input_and_near_overflow_lengths_do_not_panic() {
    let mut state = 0x1234_5678u32;
    for length in 0..2048 {
        let mut b = vec![0; length];
        for byte in &mut b {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
        let _ = Package::parse(&b);
        let _ = Index::parse(&b);
        let _ = PolicyDocument::parse(&b);
    }
    let data = app();
    for value in [u64::MAX, u64::MAX - 191, MAX_PACKAGE_BYTES + 1] {
        let mut b = data.clone();
        put64(&mut b, 16, value);
        put64(&mut b, 96, value.wrapping_sub(192));
        assert!(Package::parse(&b).is_err());
    }
}

fn unhex(text: &str) -> Vec<u8> {
    let text = text.trim();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
#[test]
fn frozen_python_ed25519_vectors_match_independent_rust_serialization() {
    let data = unhex(include_str!("vectors/application.hex"));
    assert_eq!(data, app());
    assert_eq!(
        digest(&data).to_vec(),
        unhex("5bc839a5403da64fef45bdb5f26925f050c7efa1ed09c3f7e92fa52bd0fea17b")
    );
    let public = unhex(include_str!("vectors/public-key.hex"));
    assert_eq!(public, key().verifying_key().to_bytes());
    let repo = unhex(include_str!("vectors/repository.hex"));
    assert_eq!(
        digest(&repo).to_vec(),
        unhex("a4bb395be74366778e226644ee33d35123ec30dc704759b703d528008142b2de")
    );
    let keys = [trust(Domain::Application), trust(Domain::Repository)];
    let policy = TrustPolicy {
        epoch: 3,
        keys: &keys,
    };
    let verified = Package::parse(&data).unwrap().verify(&policy).unwrap();
    let parsed = Index::parse(&repo).unwrap();
    parsed
        .verify(
            &policy,
            Some(500),
            &RepositoryState {
                id: "dev.example.repository",
                sequence: 0,
                digest: [0; 32],
            },
        )
        .unwrap();
    parsed.entries().next().unwrap().bind(&verified).unwrap();
}

#[test]
fn base_container_metadata_is_bounded_without_application_id_or_filename_tables() {
    let mut bytes = vec![0; 96];
    bytes[..8].copy_from_slice(b"OSABASE1");
    put32(&mut bytes, 8, 1);
    put32(&mut bytes, 12, 1);
    put64(&mut bytes, 16, 160);
    put32(&mut bytes, 32, 1);
    put64(&mut bytes, 40, 96);
    put64(&mut bytes, 48, 64);
    bytes[56..88].fill(1);
    put32(&mut bytes, 88, 1);
    assert_eq!(
        system::BaseMetadata::parse(&bytes, 160)
            .unwrap()
            .service_count(),
        1
    );
    for (offset, value) in [(12, 33), (32, 0), (32, 33), (36, 1), (88, 0)] {
        let mut bad = bytes.clone();
        put32(&mut bad, offset, value);
        assert!(system::BaseMetadata::parse(&bad, 160).is_err());
    }
    for (offset, value) in [(16, u64::MAX), (40, 95), (40, u64::MAX), (48, u64::MAX)] {
        let mut bad = bytes.clone();
        put64(&mut bad, offset, value);
        assert!(system::BaseMetadata::parse(&bad, 160).is_err());
    }
    for len in 0..bytes.len() {
        assert!(system::BaseMetadata::parse(&bytes[..len], 160).is_err());
    }
}
