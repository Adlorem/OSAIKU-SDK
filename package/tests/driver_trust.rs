#[path = "support/driver_fixture.rs"]
mod fixture;
use osaiku_package::{
    driver::{self, TrustClass},
    policy::PolicyDocument,
    Error,
};

#[test]
fn valid_controller_signature_and_system_name_do_not_grant_catalog_authority() {
    let policy = fixture::policy(2, false, &[4, 5, 6, 7]);
    let policy = PolicyDocument::parse(&policy).unwrap();
    for id in [
        "dev.osaiku-drivers.nvme",
        "dev.osaiku-drivers.xhci",
        "third.party",
    ] {
        for classes in [4, 6] {
            let bytes = fixture::package(id, 1, classes, &[], false);
            let package = driver::parse(&bytes)
                .unwrap()
                .verify(&policy.policy())
                .unwrap();
            assert_eq!(
                TrustClass::Installable.admit(package.manifest()),
                Err(Error::Policy)
            );
            assert_eq!(TrustClass::System.admit(package.manifest()), Ok(()));
        }
    }
}

#[test]
fn mediated_classes_remain_installable_without_system_status() {
    for classes in [1, 2, 3] {
        let bytes = fixture::package("third.party", 1, classes, &[], false);
        let package = driver::parse(&bytes).unwrap();
        assert_eq!(TrustClass::Installable.admit(package.manifest()), Ok(()));
    }
}
