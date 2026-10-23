use osaiku_runtime::package_compatibility::*;

const PROFILE: Profile = Profile {
    architecture: X86_64,
    process_abi: Version { major: 1, minor: 2 },
    cpu_features: cpu::SSE3,
};
const REQUIRED: ProtocolRequirement = ProtocolRequirement {
    id: 7,
    version: Version { major: 3, minor: 4 },
    optional: false,
};
// Independently specified wire vector: 32-byte header, one 16-byte record.
const VECTOR: [u8; 48] = [
    1, 0, 62, 0, 1, 0, 2, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 7, 0, 0, 0, 3, 0, 4, 0, 1, 0, 0, 0, 0, 0, 0, 0,
];

fn bytes(profile: Profile, protocols: &[ProtocolRequirement]) -> Vec<u8> {
    let mut output = vec![0; MAX_BYTES];
    let len = encode(profile, protocols, &mut output).unwrap();
    output.truncate(len);
    output
}

#[test]
fn canonical_vector_has_no_native_layout_or_sdk_version_dependency() {
    assert_eq!(bytes(PROFILE, &[REQUIRED]), VECTOR);
    let record = Requirements::decode(&VECTOR).unwrap();
    assert_eq!(record.profile(), PROFILE);
    assert_eq!(record.protocols().collect::<Vec<_>>(), [REQUIRED]);
    let mut output = [0xaa; 80];
    assert_eq!(encode(PROFILE, &[REQUIRED], &mut output), Ok(48));
    assert_eq!(&output[48..], &[0xaa; 32]);
}

#[test]
fn truncated_extended_and_oversized_records_are_refused() {
    for len in 0..VECTOR.len() {
        assert!(
            Requirements::decode(&VECTOR[..len]).is_err(),
            "length {len}"
        );
    }
    let mut extended = VECTOR.to_vec();
    extended.push(0);
    assert_eq!(
        Requirements::decode(&extended).unwrap_err(),
        DecodeError::Length
    );
    assert_eq!(
        Requirements::decode(&[0; MAX_BYTES + 1]).unwrap_err(),
        DecodeError::Length
    );
    for count in [0u16, 2, 32, 33, u16::MAX] {
        let mut bad = VECTOR;
        bad[16..18].copy_from_slice(&count.to_le_bytes());
        assert!(Requirements::decode(&bad).is_err());
    }
}

#[test]
fn every_reserved_byte_and_unknown_flag_is_rejected() {
    for index in (18..32).chain(44..48) {
        let mut bad = VECTOR;
        bad[index] = 1;
        assert_eq!(
            Requirements::decode(&bad).unwrap_err(),
            DecodeError::Reserved
        );
    }
    for flags in [2u32, 3, 0x8000_0000, u32::MAX] {
        let mut bad = VECTOR;
        bad[40..44].copy_from_slice(&flags.to_le_bytes());
        assert_eq!(
            Requirements::decode(&bad).unwrap_err(),
            DecodeError::ProtocolFlags
        );
    }
    for (offset, value, error) in [
        (0, 2u16, DecodeError::Version),
        (2, 0, DecodeError::Profile),
        (4, 0, DecodeError::Profile),
        (36, 0, DecodeError::ProtocolVersion),
    ] {
        let mut bad = VECTOR;
        bad[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        assert_eq!(Requirements::decode(&bad).unwrap_err(), error);
    }
}

#[test]
fn duplicate_unsorted_and_zero_protocol_ids_are_not_canonical() {
    let second = ProtocolRequirement { id: 8, ..REQUIRED };
    for protocols in [
        vec![REQUIRED, REQUIRED],
        vec![second, REQUIRED],
        vec![ProtocolRequirement { id: 0, ..REQUIRED }],
    ] {
        let mut output = [0xaa; MAX_BYTES];
        assert_eq!(
            encode(PROFILE, &protocols, &mut output),
            Err(DecodeError::ProtocolOrder)
        );
        assert_eq!(output, [0xaa; MAX_BYTES]);
    }
    for id in [0u32, 6, 7] {
        let mut bad = bytes(PROFILE, &[REQUIRED, second]);
        bad[48..52].copy_from_slice(&id.to_le_bytes());
        assert_eq!(
            Requirements::decode(&bad).unwrap_err(),
            DecodeError::ProtocolOrder
        );
    }
}

#[test]
fn process_abi_architecture_and_usable_cpu_features_are_independent_gates() {
    let data = bytes(PROFILE, &[]);
    let requirements = Requirements::decode(&data).unwrap();
    for (profile, expected) in [
        (PROFILE, Ok(())),
        (
            Profile {
                process_abi: Version { major: 1, minor: 3 },
                ..PROFILE
            },
            Ok(()),
        ),
        (
            Profile {
                process_abi: Version { major: 1, minor: 1 },
                ..PROFILE
            },
            Err(CompatibilityError::ProcessAbi),
        ),
        (
            Profile {
                process_abi: Version { major: 2, minor: 9 },
                ..PROFILE
            },
            Err(CompatibilityError::ProcessAbi),
        ),
        (
            Profile {
                cpu_features: 0,
                ..PROFILE
            },
            Err(CompatibilityError::CpuFeatures),
        ),
        (
            Profile {
                architecture: 183,
                ..PROFILE
            },
            Err(CompatibilityError::InvalidPlatform),
        ),
        (
            Profile {
                cpu_features: cpu::AVX2,
                ..PROFILE
            },
            Err(CompatibilityError::InvalidPlatform),
        ),
    ] {
        assert_eq!(
            requirements
                .check(&Platform {
                    profile,
                    protocols: &[]
                })
                .map(|_| ()),
            expected
        );
    }
    for (profile, error) in [
        (
            Profile {
                architecture: 183,
                ..PROFILE
            },
            CompatibilityError::Architecture,
        ),
        (
            Profile {
                cpu_features: 1 << 63,
                ..PROFILE
            },
            CompatibilityError::CpuFeatures,
        ),
    ] {
        let data = bytes(profile, &[]);
        let req = Requirements::decode(&data).unwrap();
        assert_eq!(
            req.check(&Platform {
                profile: PROFILE,
                protocols: &[]
            }),
            Err(error)
        );
    }
}

#[test]
fn unavailable_required_protocol_refuses_but_optional_is_reported_absent() {
    for optional in [false, true] {
        let data = bytes(
            PROFILE,
            &[ProtocolRequirement {
                optional,
                ..REQUIRED
            }],
        );
        let req = Requirements::decode(&data).unwrap();
        for (protocols, expected) in [
            (vec![], false),
            (
                vec![ProtocolSupport {
                    id: 8,
                    version: REQUIRED.version,
                }],
                false,
            ),
            (
                vec![ProtocolSupport {
                    id: 7,
                    version: Version { major: 2, minor: 9 },
                }],
                false,
            ),
            (
                vec![ProtocolSupport {
                    id: 7,
                    version: Version { major: 3, minor: 3 },
                }],
                false,
            ),
            (
                vec![ProtocolSupport {
                    id: 7,
                    version: REQUIRED.version,
                }],
                true,
            ),
            (
                vec![ProtocolSupport {
                    id: 7,
                    version: Version { major: 3, minor: 5 },
                }],
                true,
            ),
        ] {
            let result = req.check(&Platform {
                profile: PROFILE,
                protocols: &protocols,
            });
            if optional || expected {
                let availability = result.unwrap();
                assert_eq!(availability.contains(0), expected);
                assert!(!availability.contains(1));
                assert!(!availability.contains(usize::MAX));
            } else {
                assert_eq!(result, Err(CompatibilityError::RequiredProtocol(7)));
            }
        }
    }
}

#[test]
fn invalid_host_offers_never_mask_missing_requirements() {
    let req = Requirements::decode(&VECTOR).unwrap();
    let offer = ProtocolSupport {
        id: REQUIRED.id,
        version: REQUIRED.version,
    };
    for protocols in [
        vec![offer, offer],
        vec![ProtocolSupport { id: 8, ..offer }, offer],
        vec![ProtocolSupport { id: 0, ..offer }],
        vec![ProtocolSupport {
            version: Version { major: 0, minor: 0 },
            ..offer
        }],
        vec![offer; MAX_PROTOCOLS + 1],
    ] {
        assert_eq!(
            req.check(&Platform {
                profile: PROFILE,
                protocols: &protocols
            }),
            Err(CompatibilityError::InvalidPlatform)
        );
    }
}

#[test]
fn maximum_record_and_last_availability_bit_work_without_overflow() {
    let protocols: Vec<_> = (1..=32)
        .map(|id| ProtocolRequirement {
            id,
            optional: id % 2 == 0,
            ..REQUIRED
        })
        .collect();
    let offered: Vec<_> = protocols
        .iter()
        .filter(|p| p.id != 2)
        .map(|p| ProtocolSupport {
            id: p.id,
            version: p.version,
        })
        .collect();
    let data = bytes(PROFILE, &protocols);
    assert_eq!(data.len(), MAX_BYTES);
    let req = Requirements::decode(&data).unwrap();
    let available = req
        .check(&Platform {
            profile: PROFILE,
            protocols: &offered,
        })
        .unwrap();
    for i in 0..32 {
        assert_eq!(available.contains(i), i != 1);
    }
    assert!(!available.contains(32));
    let mut too_many = protocols;
    too_many.push(ProtocolRequirement { id: 33, ..REQUIRED });
    let mut output = [0xaa; MAX_BYTES];
    assert_eq!(
        encode(PROFILE, &too_many, &mut output),
        Err(DecodeError::ProtocolLimit)
    );
    assert_eq!(output, [0xaa; MAX_BYTES]);
}

#[test]
fn short_output_is_untouched_and_arbitrary_input_never_panics() {
    let mut output = [0xaa; 47];
    assert_eq!(
        encode(PROFILE, &[REQUIRED], &mut output),
        Err(DecodeError::Length)
    );
    assert_eq!(output, [0xaa; 47]);
    let mut state = 0x1234_5678u32;
    for length in 0..=MAX_BYTES + 1 {
        let mut data = vec![0; length];
        for byte in &mut data {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = state as u8;
        }
        let _ = Requirements::decode(&data);
    }
}
