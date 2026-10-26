# Driver packages v1 (DRV-07)

Driver packages use `.osadrv`, envelope magic `OSADRV01`, version 1 and the
signature domain `OSAIKU/driver-package/v1\0`. They use the same 192-byte envelope,
Ed25519 verification, SHA-256 content identity, bounded streaming verifier and
component layout as applications. The host chooses the expected domain. An
application parser rejects a driver package, and a driver parser rejects an
application even when it claims a driver name or role. Old readers refuse the
new magic/domain; application v1/v2 serialization is unchanged.

The public authoring tool is `tools/package.py`; `package-check` is built from
this SDK alone (`cargo build -p osaiku-package --example package-check`).

```toml
[package]
id = "example.usb-status"
name = "USB status"
version = "1.0"
release = 1
epoch = 1
executable = "driver.elf"
memory_bytes = 262144
io_bytes = 65536
permissions = 0
threads = 1
instances = 1

[compatibility]
abi_major = 1
cpu_features = 0

[driver]
classes = 8
cpu_permille = 50
irq_per_second = 0
queue = 8
isolation = "mediated"
reset = "function"

[[matches]]
bus = "usb"
class = 3

[[resources]]
name = "firmware-main"
path = "firmware.bin"
kind = "data"
```

`resources` is optional. Firmware is a signed named component, at most 1 MiB
each; its source pathname is not embedded as runtime authority. Reads must use
the exact approved immutable revision and component name. There is no path
lookup, network fetch or flashing operation in the firmware API.

```sh
python tools/package.py --checker /path/package-check driver-pack driver.toml \
  --key /outside-checkout/private-seed --output driver.osadrv
python tools/package.py --checker /path/package-check driver-inspect driver.osadrv
python tools/package.py --checker /path/package-check driver-verify driver.osadrv \
  --policy policy.bin
python tools/package.py --checker /path/package-check driver-key \
  --public-key publisher.pub --classes 8 --output publisher.osadkey
```

The key request contains only the public key and requested sensitive roles.
It is not a grant. Import it with the protected Device Manager file chooser and
approve the displayed fingerprint and roles, then inspect/install `.osadrv`.
An ordinary application cannot submit an approval or change the expected domain.
`driver-key --revoke` produces an explicit revocation request. Confirming it
advances the durable policy epoch and retains revoked key records. A revoked key
cannot be resurrected by importing the public key again.

Policy domains are Application=1, Repository=2, System=3, Driver=4,
DriverInput=5, DriverStorage=6, DriverController=7, DriverRepository=8. A driver always needs domain 4.
Class bits 1 (input), 2 (storage) and 4 (controller) additionally need the
corresponding sensitive domain for the same publisher and signing epoch.
Bits 8, 16 and 32 identify other mediated classes and require domain 4; these
bits are classification, not a raw hardware grant. Application/repository/system
keys are never implicitly promoted. The six resource-request bits are reserved
for owner admission; signing them is not permission to access hardware.

The driver manifest is `OSADMF01`, version 1. Its 512-byte prefix shares the
bounded package identity, release, resource request, RAM/I/O/instance limits and
compatibility fields with the common executable manifest. The driver extension
occupies bytes 448..474: classes u64, CPU permille u32 (1..1000), IRQ/s u32
(0..100000), queue u16 (1..64), isolation u16 (mediated=1, IOMMU=2), reset u16
(function=1, group=2, cold=3), match count u16 and dependency count u16. Remaining
prefix bytes are zero. Driver entry is headless; windows and icons are refused.

After the aligned compatibility record are at most 16 match records (32 bytes)
and 16 dependency records (208 bytes), then common 112-byte component records.
Match records contain seven u16 fields: bus (PCI=1, USB=2), vendor, product,
class, subclass, protocol, interface. `65535` is a wildcard; remaining bytes
are zero and records are strictly ordered. The bus owner supplies observations.
Dependencies name exact publisher hash (32 bytes), package ID (128), release
(u64), package digest (32), and eight zero bytes, ordered by publisher/ID.
Missing providers, out-of-set references, cycles and duplicate selections fail
before a new selection is published.

OS storage uses the existing authenticated OSAIFS package owner selected with
`Domain::Driver`, its streaming stage/verify/seal/publish transaction, immutable
objects, receipts and pins. Driver catalog/policy and activation records use
reserved `app-store-driver-*` names. Application collection excludes this
namespace, and ordinary file capabilities cannot access it. There is one
charged owner for each domain per mounted volume, outside the manager process.

Installation creates an inactive retained version. A separate protected choice
binds full digest, requested resources/classes, policy epoch and catalog revision.
The activation catalog atomically publishes a consistent selection and monotonic
generation. States include Installed, NoDevice, PendingRestart, Trial, Active,
Disabled and Removed. The protected broker records NoDevice or PendingRestart until a supported
class owner has admitted the exact selection. DRV-08 connects Boot HID Input
packages to the shared charged image reader/ELF loader, process handshake and
Trial/Active confirmation. Other hardware classes remain pending their migration.
An unlaunched package is never reported as active. Internal APIs for trial, confirmation and launch
pinning reverify exact bytes and current trust, including dependency versions.

Previous versions remain retained during trial. Rollback needs an explicit
approval of the exact older digest and cannot bypass revoked keys or a minimum
release. Reusing a publisher/package/release with different content is refused,
including tombstones after uninstall. Deactivation closes admission; uninstall
preserves live pins and protects boot/recovery versions and selected dependents.
Collection retires bytes only after pins drain. Reinstallation gets a new object
and activation generation. The bounded v1 activation catalog holds 32 version
records including removal tombstones; exhaustion is a refusal, not history loss.

Each COW publication is authenticated and read back before the durable root.
Uncertain publication poisons the owner. Restart reconstructs committed state;
an interrupted upload is never replayed or activated. A committed package whose
activation registration was interrupted remains retained and inactive. Reopening
it may register that exact revision. A missing/corrupt policy never trusts a
fixture key. Read-only/degraded OSAIFS refuses installation; this does not close
the separate A2-R writable-recovery qualification.

Tests reside in `package/tests/driver_packages.rs`,
`tools/test_driver_package.py`, and the OS's `abi/tests/support/osaifs_v3_drivers.rs`.
Production code contains no test entry points, fault switches or inline tests.


DRV-09 adds match bus `i8042` (wire value 3). Trusted facts are
`[0, 0, class, 0, 0, 0]`, where class 1 is the keyboard and class 2 is AUX.
Only the controller's successful presence/bring-up observation creates these
facts; a package match cannot create a device. PCI/USB match values are unchanged.
The installed PS/2 clients use the same signed catalog, lease, image loader,
input-role publisher policy and bounded process supervisor as USB HID.

DRV-13 embeds release-owned boot/recovery packages in authenticated RAM before
mounting the disk catalog. See [early-drivers-v1.md](early-drivers-v1.md) for
the generic role-31 format, dependency pins and bank lifetime.

## Device Manager and repository configuration (DRV-15)

Device Manager owns driver installation, admitted version selection, rollback,
attachment controls and diagnostics. Package Manager accepts application files
only. File Manager routes `.osadrv` and `.osadkey` to Device Manager; the extension
selects the owner, and the shared parser checks the actual bytes. Existing driver
catalogs and consents are used in place. Committing bytes does not start a driver.
Early/recovery packages remain visible as protected system-release entries;
changing that set requires an authenticated system update and reboot.

`osaiku_runtime::devices` defines versioned redacted snapshots and proposals.
The protected manager and an admitted assistant may read facts or request a
proposal against an exact snapshot stamp. A proposal carries confirmation-needed
status and does not execute hardware operations, modify trust or admit code.
Disabling supported noncritical input applies to one attachment and requires a
working alternative of that input class; reconnect restores the admitted policy.
Controller/storage/boot-set changes require their protected lifecycle policy.

Source configuration is independent for applications and drivers. Create a
512-byte local configuration with a repository public key (not a publisher grant):

```toml
[source]
domain = "driver"
id = "example.drivers"
address = "https://packages.example/drivers/stable.index"
public_key = "repository.pub"
channel = "stable"
priority = 100
enabled = true
minimum_epoch = 1
architecture = 62
abi_major = 1
abi_minor = 0
```

```sh
python tools/package.py --checker /path/package-check source-pack source.toml \
  --output stable.osadsrc
python tools/package.py --checker /path/package-check driver-index-pack index.toml \
  --key /outside-checkout/repository-seed --output stable.osadidx
python tools/package.py --checker /path/package-check driver-index-inspect stable.osadidx
```

The index manifest uses the existing `[repository]` and `[[entries]]` format,
but each entry must name a driver package. Driver indexes use `OSADIX01` and
`OSAIKU/driver-repository-index/v1\0`; application indexes retain `OSAIDX01`.
For application sources use `domain = "application"` and `.osasrc`.
Import configuration via the corresponding manager's file chooser, review the
complete origin, fingerprint, channel, compatibility and priority, then confirm.
Each owner supports eight sources and explicit enable/disable/removal. Source
changes use a durable revision; stale confirmations are refused. Removing a
source does not remove installed packages or revoke their publishers.

Configured-source checkpoints are persisted before returning update candidates.
Domain, origin/configuration identity, authenticated index and package digest
bind candidates and cache keys. The configured key epoch is independent of the
publisher policy; admitting a new repository signing epoch requires an explicit
source configuration update. Repository keys cannot authorize driver/input/storage
packages. The fixed-capacity checkpoint table retains anti-rollback history and
refuses exhaustion instead of silently dropping old checkpoints.

These are local metadata contracts. No HTTPS fetching, online search or background
installation is implemented here; the common transport and its independent domain
queues require Stage 33 acceptance. No local fixture qualifies network retrieval.
