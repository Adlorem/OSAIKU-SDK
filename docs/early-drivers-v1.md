# Immutable early driver set v1

DRV-13 adds generic base role **31**, ABI 1, before ordinary catalog role 32.
The revision-7 handoff and four bank files are unchanged. A system release
signature binds the whole base and kernel, while each nested driver retains
its existing driver signature and sensitive publisher domain. A disk policy
does not replace the release owner's early-driver trust policy.

`package::system::drivers` provides bounded RAM parsing and streaming
pre-mutation verification. All integers are little endian. The 64-byte header
contains `OSADSET1` at 0, version u32 at 8, count u32 at 12, total bytes u64 at
16 and system service ABI u64 at 24. Bytes 32..64 are zero. There are 1..16
entries; the entire object is at most 16 MiB.

Each 64-byte entry contains flags u32 at 0, dependency mask u32 at 4, offset
u64 at 8, length u64 at 16 and SHA-256 at 24. Bytes 56..64 are zero. Bodies
are contiguous in table order with zero padding to eight-byte boundaries;
there are no trailing bytes. Each body is a complete `.osadrv`, including
its executable and declared firmware. Offsets are relative to the set.

Flags are CRITICAL=1 and RECOVERY=2. Recovery implies critical. Unknown flags
are rejected. Dependency bits refer only to earlier entries, so cycles and
forward references fail. The mask must exactly match the signed manifest's
publisher, package ID, release and digest dependencies. Dependencies inherit
all caller flags. Repeated publisher/package identity is rejected.

Parsing alone grants no authority. `Set::verify` and `verify_stream` check
each complete nested package with the caller's trusted policy, including all
components. An envelope signed only for ordinary applications cannot become
a driver. The OS accepts process ABI major 1 and checks the actual platform
requirements again at launch. Legacy bases without role 31 remain readable
for rollback; new release authoring requires an explicit set.

The OS retains the authenticated base in reserved RAM and uses the common
W^X process loader without opening OSAIFS. Critical entries start only on a
matching admitted interface. Dependencies must be ready first. Missing
hardware does not fabricate an instance. xHCI additionally needs the DRV-11
platform session; packaging it grants no controller or DMA authority.

Release-owned instances reserve their class/controller through reboot, even
after failure. The disk catalog cannot replace them with newer entries or
terminate them when its own policy is unavailable. Removing/deactivating a
critical identity through the package broker is refused. Optional set entries
are authenticated but not automatically installed or resurrected in the disk
catalog. Boot-critical updates replace the whole base in the existing inactive
bank, retaining the confirmed bank and its dependencies. Catalog GC has no
ownership of these RAM/ESP bytes.

Trial health requires critical instance liveness plus the existing trusted
storage, input and service observations. A process READY message alone cannot
confirm a bank. Runtime signatures do not authenticate the running EFI image
or firmware root: the current profile remains **development-unverified**.

Tests live in `package/tests/early_drivers.rs` and the OS's separate installer
and QEMU test files. There are no production test methods or fault switches.
