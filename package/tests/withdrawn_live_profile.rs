use osaiku_package::{system::*, Error};

// Structurally valid metadata: interpretation of the removed data role must
// still fail even if an older system release authenticated its container.
fn metadata(extra: &[u32]) -> (Vec<u8>, u64) {
    let mut roles = BASE_PROFILE_ROLES.to_vec();
    roles.extend(extra);
    roles.sort();
    let mut bytes = vec![0u8; BASE_HEADER_BYTES + BASE_ENTRY_BYTES * roles.len()];
    bytes[..8].copy_from_slice(b"OSABASE1");
    bytes[8..12].copy_from_slice(&BASE_VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(roles.len() as u32).to_le_bytes());
    let mut offset = bytes.len() as u64;
    for (i, role) in roles.iter().enumerate() {
        let at = BASE_HEADER_BYTES + i * BASE_ENTRY_BYTES;
        bytes[at..at + 4].copy_from_slice(&role.to_le_bytes());
        bytes[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
        bytes[at + 16..at + 24].copy_from_slice(&8u64.to_le_bytes());
        bytes[at + 24..at + 56].fill(1);
        bytes[at + 56..at + 60].copy_from_slice(&1u32.to_le_bytes());
        offset += 8;
    }
    bytes[16..24].copy_from_slice(&offset.to_le_bytes());
    (bytes, offset)
}
#[test]
fn removed_live_role_is_not_an_optional_service() {
    for extra in [vec![30], vec![30, 31], vec![22, 30, 31]] {
        let (b, len) = metadata(&extra);
        assert_eq!(
            BaseMetadata::parse(&b, len).unwrap().validate_profile(),
            Err(Error::Compatibility)
        );
    }
}
#[test]
fn normal_early_driver_and_diagnostic_profiles_remain_supported() {
    for extra in [vec![], vec![31], vec![22, 31]] {
        let (b, len) = metadata(&extra);
        BaseMetadata::parse(&b, len)
            .unwrap()
            .validate_profile()
            .unwrap();
    }
}
