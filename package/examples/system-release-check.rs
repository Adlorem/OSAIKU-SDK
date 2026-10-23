//! Independent release validation with the exact no_std consumers used at boot.
use osaiku_package::{
    application::Package,
    system::{self, BaseMetadata, Release},
    *,
};
use std::{fs, path::Path};
fn read(path: &Path) -> Vec<u8> {
    let n = fs::metadata(path).expect("input metadata").len();
    assert!(n > 0 && n <= MAX_PACKAGE_BYTES);
    fs::read(path).expect("input bytes")
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "system-release-check EFI/BOOT PUBLIC_KEY");
    let root = Path::new(&args[1]);
    let public: [u8; 32] = read(Path::new(&args[2]))
        .try_into()
        .expect("32-byte public key");
    let keys = [Domain::System, Domain::Application].map(|domain| TrustedKey {
        public_key: public,
        domain,
        minimum_epoch: 1,
        revoked: false,
    });
    let policy = TrustPolicy {
        epoch: 1,
        keys: &keys,
    };
    let release = read(&root.join("RELEASE.SIG"));
    let kernel = read(&root.join("OSAIKERN.ELF"));
    let bytes = read(&root.join("OSAIBASE.PKG"));
    let release = Release::parse(&release).expect("release structure");
    release.verify(&policy).expect("release signature");
    release
        .verify_objects(&kernel, &bytes)
        .expect("release pair");
    let base = BaseMetadata::validate_object(&bytes).expect("base integrity");
    base.validate_profile().expect("service profile");
    let e = base
        .entries()
        .find(|e| e.role == system::catalog::ROLE)
        .unwrap();
    let catalog =
        system::catalog::Catalog::parse(&bytes[e.offset as usize..(e.offset + e.length) as usize])
            .expect("catalog");
    let bodies = read(&root.join("APPS.PKG"));
    assert_eq!(bodies.len() as u64, catalog.bundle_bytes());
    assert_eq!(digest(&bodies), catalog.bundle_digest());
    for entry in catalog.entries() {
        let data = &bodies[entry.offset() as usize..(entry.offset() + entry.length()) as usize];
        let package = Package::parse(data).expect("ordinary package");
        let verified = package.verify(&policy).expect("application authentication");
        assert_eq!(verified.sha256(), entry.digest());
        assert_eq!(&data[..entry.metadata().len()], entry.metadata());
        println!(
            "package={} bytes={} role=ordinary",
            verified.manifest().id(),
            data.len()
        );
    }
    println!(
        "release={} handoff=7 base_bytes={} disk_app_bytes={}",
        release.generation(),
        bytes.len(),
        bodies.len()
    );
}
