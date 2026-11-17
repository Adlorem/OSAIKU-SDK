# Driver manifest v3: embedded publisher key

Set `include_public_key = true` in `[driver]` when using `tools/package.py`.
The authoring tool derives the public Ed25519 key from the signing seed. The
seed remains outside the package. Omitting the option preserves v1/v2 output.

The `OSADRV01` envelope remains version 1 (192 bytes). `OSADMF01` becomes
version 3. The 32-byte public key occupies manifest bytes 480..512 and is part
of the signed payload. Bytes 476..480 remain zero. Invalid/weak keys and a
publisher identity different from SHA-256(public key) are rejected. Old readers
reject v3; application formats are unchanged.

For classes exactly 64, v3 preserves the complete [GPU v2](gpu-packages-v2.md)
contract, two executables, revision-aware matches and contract length at 474.
Other driver classes preserve [v1](driver-packages-v1.md) component/match rules
and require zero bytes 474..480. A GPU revision bound cannot be lost through the
legacy matching API.

An embedded public key is **not trust**. Normal verification and runtime
admission still require the installed publisher policy. `driver-inspect` reports
key presence and `trust-granted=false`; `driver-verify` still requires a policy.
`driver::installation::Proposal` builds only a temporary candidate policy for
complete signature/content verification. It grants no runtime capabilities.

The protected host displays the verified publisher fingerprint, package digest,
length, release, resources and classes. If required trust is missing, an explicit
“Zaufaj wydawcy i zainstaluj” action binds that snapshot. The host reopens and
verifies the full source, checks the exact key, digest, length, requested classes,
permissions, live owner, mount, catalog and policy epoch, then persists only the
missing Driver and required sensitive class domains. Existing trust needs normal
install/update consent. Revoked keys and minimum signing epochs remain binding;
neither opening a file nor reimporting its key restores revoked trust.

Trust publication and package installation are separate durable operations.
If installation fails after confirmed trust publication, the approved publisher
may remain trusted. No package is automatically selected or launched. Normal
staging verifies all bytes again under the persisted policy. Cancellation before
confirmation does not provision policy or persist trust. OSAIFS remains required.
Legacy `.osadkey` import/revocation and v1/v2 packages remain supported.

Tests: `package/tests/embedded_driver_key.rs`,
`tools/test_embedded_driver_key.py`, and the OS production-storage tests in
`abi/tests/support/osaifs_v3_embedded_driver_key.rs` (including an explicit real
delivery test selected with `OSAIKU_EMBEDDED_DRIVER_PACKAGE`).
