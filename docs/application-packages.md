# Application packages v1

`osaiku-package` is the single `no_std`, allocation-free source for package,
compatibility, repository, publisher-policy and system-release validation. It
has no OS checkout dependency and no private-key loading code. `osaiku-runtime`
reexports compatibility types; the OS reexports exactly those same types.
The host Python serializer is checked against the shared Rust parser before
publishing output. It is not a second verifier.

These tools create and verify artifacts. They do not install or run them.
Package signatures do not grant capabilities or system roles. Existing Themes
can be packaged as a migration candidate, but its runtime still needs migration
to the public appearance/file protocols. The native-hello example builds solely
from SDK sources; it is not a Linux executable.

## Tools

Use the pinned Rust toolchain and Python 3.11+. Signing requires the dependencies
in `tools/requirements-package.txt`. Inspect/verify do not import private-key code
or require cryptography. From the SDK root, build the shared checker:

```text
cargo build --locked -p osaiku-package --example package-check
python tools/package.py --checker target/debug/examples/package-check pack examples/native-hello/package.toml --executable path/to/osaiku-native-hello --key /private/location/publisher.seed --output hello.osapp
python tools/package.py --checker target/debug/examples/package-check inspect hello.osapp
python tools/package.py --checker target/debug/examples/package-check trust-add --policy approved.policy --public-key publisher.pub --fingerprint EXACT_SHA256_OF_PUBLIC_KEY --domain application --epoch 1
python tools/package.py --checker target/debug/examples/package-check verify hello.osapp --policy approved.policy
```

On Windows the checker has `.exe`. An OS checkout may configure a different
Cargo target directory; pass that actual checker path. Seed and public key
files contain exactly 32 raw bytes each. Generate keys with a trusted Ed25519
implementation, keep seeds outside repository checkouts, and approve the public
key fingerprint independently. Packages contain only the fingerprint, not a
trusted key. The tool refuses private-key paths inside SDK or containing
repository roots. The committed test key/vectors are public test material and
must never be approved for production. Test key files are ephemeral.

`trust-add` requires an explicit matching fingerprint and a strictly increasing
policy epoch. It does not accept a key embedded in an application. Repository
keys require separate approval with `--domain repository`; system keys use
`system`. The same mathematical key approved for one domain has no authority
in another. `trust-revoke` uses the same explicit public-key arguments and a
new epoch. Revoked entries cannot silently be reapproved. Policy edits use an
exclusive lock and atomically replace the validated record. Interrupted locks
cause refusal; an administrator may remove a stale lock after checking no editor
is active. Policy files are trusted host configuration, not download formats.
Runtime storage must protect them and persist epoch floors against rollback.

`verify --release-floor N` rejects older releases unless a host-approved
`--rollback-digest` names precisely that complete package SHA-256. The CLI does
not infer that approval or update a runtime catalog. A catalog must additionally
reject a different digest for an already published release and bind approval to
its current identity, policy and operation generation.

## Repository authoring

```toml
[repository]
id = "dev.example.repository"
sequence = 1
issued_at = 1792627200
expires_at = 1792713600
epoch = 2

[[entries]]
package = "hello.osapp"
url = "https://packages.example/hello.osapp"
```

```text
python tools/package.py --checker target/debug/examples/package-check index-pack repository.toml --key /private/location/repository.seed --output catalog.osaidx
python tools/package.py --checker target/debug/examples/package-check index-inspect catalog.osaidx
python tools/package.py --checker target/debug/examples/package-check index-verify catalog.osaidx --policy approved.policy --now TRUSTED_UTC_SECONDS --repository dev.example.repository --sequence-floor 0 --checkpoint 0000000000000000000000000000000000000000000000000000000000000000 --package hello.osapp
```

Zero sequence/checkpoint is for a newly approved repository only. Persist the
returned sequence and digest before using its entries; pass the persisted values
on later checks. Equal sequence requires identical bytes; a smaller sequence is
rejected. The index must not be from the future or expired, and its validity
interval is at most seven days. Missing trusted time is an error. `--package`
also checks the selected package's publisher policy and exact entry binding;
repository trust alone never approves a publisher.

V1 URLs are HTTPS on port 443, a lowercase DNS hostname and a nonempty ASCII
path using alphanumerics, `-._~/`. Empty/dot path segments, credentials, query
strings, fragments, percent escapes, IP literals, explicit ports and redirects
are unsupported. The runtime broker must also enforce approved destinations
after DNS resolution and validate TLS 1.3 hostname, chain and certificate times.
These authoring tools do not perform network I/O.

## Canonical binary format

All integers are little-endian; strings occupy fixed fields, with nonempty UTF-8
content and zero padding (no embedded NUL, control or bidirectional override
characters). Package IDs are at most 128 ASCII bytes: dot-separated components
start with a lowercase letter, end alphanumeric and contain lowercase letters,
digits or hyphens. `osaiku`, `org.osaiku` and their subnamespaces, plus built-in
service/application names, are reserved. Version/SDK labels are up to 64 ASCII
alphanumeric/`._-+` bytes and have no compatibility authority.

Every object begins with a 192-byte envelope:

| Offset | Field |
| --- | --- |
| 0 | 8-byte magic: `OSAPPK01`, `OSAIDX01`, or `OSASYS01` |
| 8 / 12 | u32 version 1 / u32 header length 192 |
| 16 / 24 | u64 exact total bytes / u64 policy epoch |
| 32 / 64 | SHA-256 public-key fingerprint / SHA-256 entire payload |
| 96 | u64 payload length |
| 104–127 | zero |
| 128–191 | Ed25519 signature |

The signed message is the exact domain followed by envelope bytes `[0,128)`.
Frozen domains (including trailing NUL) are
`OSAIKU/application-package/v1\0`, `OSAIKU/repository-index/v1\0`, and
`OSAIKU/system-release/v1\0`. The signature therefore binds metadata, component
hashes, payload, epoch, identity and exact sizes. Strict Ed25519 verification is
used. The trusted policy must be at least the signed epoch and the key's minimum
accepted epoch must not exceed the signed epoch. Revocation always overrides.

Package payload begins with a 512-byte manifest, compatibility record, zero
padding to eight bytes, then 112-byte component descriptors. Metadata totals at
most 16 KiB. There are one ELF and at most 128 resources. Package size is at most
256 MiB, ELF at most 128 MiB, each ordinary resource at most 16 MiB. Components
follow in descriptor order, eight-byte aligned, with zero padding and no gaps,
overlap or trailing data beyond that alignment. Hashes cover actual component
bytes, and the envelope additionally covers all padding.

| Manifest offset | Field |
| --- | --- |
| 0 / 8 / 12 | `OSAMNF01`, u32 version 1, u32 metadata size |
| 16 / 24 | u64 monotonic release / requested permission bits |
| 32 / 40 | u64 memory budget / I/O budget |
| 48 / 50 / 52 | u16 threads (1–16), instances (1–64), surfaces (0–16) |
| 54 / 56 / 58 | u16 entry (1 native, 2 window), preferred width/height |
| 60 / 62 | u16 compatibility length / component count |
| 64 / 192 | 128-byte ID / 128-byte display name |
| 320 / 384 | 64-byte version label / 64-byte SDK label |
| 448–511 | zero |

Memory requests are 4096 through 65,535 × 4096 bytes; I/O requests are 1 through
16 MiB. Native entry has no surface or window dimensions; window entry requires
at least one surface and dimensions 64–4096. Requests never override host policy.
Permission bits 0–3 are selected-file read, selected-file write, desktop
appearance write, and system-metrics read. Unknown bits are refused; none grants
raw-disk access, arbitrary process control or a privileged role.

Component offsets: `0:u16 kind` (1 ELF, 2 resource, 3 icon), `2..8` zero,
`8:u64 absolute offset`, `16:u64 length`, `24:[u8;32] SHA-256`,
`56:[u8;56] logical name`. First entry is `executable`, kind 1. Remaining names
use the ID grammar and are strictly sorted/unique, never destination paths.
At most one icon is allowed: `RGBA`, u16 width, u16 height, straight RGBA8 pixels;
dimensions 1–256, exact length `8 + width*height*4` (max 262,152 bytes).
This avoids image decompression in package verification. The ELF identity check
requires LE ELF64 ET_EXEC x86-64; segment/BSS/W^X validation remains in the
existing process loader, which must run before execution.

Compatibility has a 32-byte header: u16 record version, architecture, process
ABI major, minimum minor; u64 CPU mask; u16 count; 14 zero bytes. Each 16-byte
requirement has u32 ID, u16 major/minimum minor, u32 required flag (0/1), and four
zeros. Up to 32 entries, strictly sorted by nonzero ID, no duplicate IDs.
Majors must match and offered minors must be at least requested. Optional
incompatibility yields absence. Protocol IDs 1–6 designate process context,
window, selected files, appearance, metrics and package catalog. Their presence
in the registry does not advertise implementation. CPU bits 0–5 mean usable
SSE3/SSSE3/SSE4.1/SSE4.2/AVX/AVX2; AVX requires host state support.

Index payload: 128-byte repository ID, u64 sequence/issued-at/expires-at at
128/136/144, u32 entry count at 152, four zeros. Up to 1024 entries, max object
1 MiB. Each 512-byte entry is publisher fingerprint (32), package ID (128),
u64 release/size, SHA-256 package (32), u16 architecture/ABI major/minor,
two zeros, and URL (296). Entries strictly sort by fingerprint then padded ID;
one current release per identity. The package's complete digest, size,
publisher, ID, release and profile must all match the chosen entry.

Policy records: `OSAPOL01`, u32 version 1/count, u64 current epoch, eight zeros
(32 bytes total), then up to 64 records of 64 bytes: public key (32), u8 domain
(1 application, 2 repository, 3 system), u8 revoked (0/1), six zeros, u64 minimum
epoch, sixteen zeros. No duplicates within a domain or weak keys. Policy has no
embedded private material. Its integrity/freshness is the trusted store's job.

System-release payload (A0, consumer implemented in A8): 128 bytes containing
u64 release, u32 handoff 7/base format 1, u64 nonzero service ABI, u64 kernel
size + SHA-256, u64 base size + SHA-256, 24 zeros. Each file is at most 256 MiB.
This commits the pair before boot; it is not a Secure Boot claim.

Base-container structural metadata is also frozen: a 32-byte header with
`OSABASE1`, u32 version 1/count (1–32), u64 exact total bytes, eight zeros;
then 64-byte service records sorted by strictly increasing role. Records carry
u32 role (1–32), four zeros, u64 offset/length, SHA-256 (32), u32 service protocol
major/minor. Component offsets are contiguous with eight-byte zero alignment,
each image at most 128 MiB, complete object at most 256 MiB. The signed release
authenticates the entire base container. Kernel service profile 1 assigns roles
1–10 to initd, logd, consoled, assistantd, policyd, serviced, inputd, compositord,
updated and modeld. The kernel must require this set and reject unsupported roles
or service majors; UEFI only validates the structural container and signed pair.
Recovery/installation are supplied by the basic services without depending on
the optional application catalog. `BaseMetadata` is the shared structural
parser; loading, complete-image/padding validation, handoff ownership, bank
transactions and physical installation remain A8/A7.

## Verification and evidence

`application::stream::Verifier` authenticates the envelope and bounded manifest
in its constructor. Feed only the remaining bytes, in order, with nonempty
chunks of at most 64 KiB, then consume it with `finish`. It checks the same
component hashes, zero padding, ELF target header and raw icon shape as
`Package::parse`, without allocating an entire package. A failed push is
terminal; truncated input cannot yield a digest. Policy must be checked again
at publication and launch. Segment/BSS/W^X validation remains the responsibility
of the common image loader; this verifier does not grant execution authority.

```text
cargo test --locked -p osaiku-package
python tools/test_package.py target/debug/examples/package-check -v
```

The frozen public vectors in `package/tests/vectors` use Python's Ed25519
implementation and independent Rust serialization/verification. Their complete
SHA-256 values are `5bc839a5403da64fef45bdb5f26925f050c7efa1ed09c3f7e92fa52bd0fea17b`
(application) and `a4bb395be74366778e226644ee33d35123ec30dc704759b703d528008142b2de`
(index). Tests also cover revocation, policy epochs, domain separation,
truncation, overflow, canonical metadata, source corruption, streaming failure,
freshness, equivocation, rollback and exact entry binding. They neither run UI
automation nor execute the test ELF header fixture. Real application builds are
separate evidence; package success alone does not prove runtime installation.

## Installed catalog observations (A5, partial)

`osaiku_runtime::packages` provides read-only syscall 395. `STATUS` obtains the
current mount, catalog revision and policy epoch; `ITEM` reads one of at most
32 active entries against that exact snapshot. Restart enumeration on `STALE`.
`OUTCOME` looks up the durable operation ID for the calling process generation;
a reused PID does not inherit another caller's receipt. Requests are 80 bytes,
replies 288 bytes; `read_with` rejects malformed or uncorrelated replies.

The kernel lazily loads the existing authenticated store on its storage pump.
A missing or invalid policy/catalog is unavailable, never implicitly seeded
with a test key. Installation, trust decisions, launch, cancellation and private
data access are not operations in this observation protocol. Their protected
runtime UI and service integration are still pending in A5; catalog success is
not evidence that runtime installation is complete. Private data has a separate
client described below. `OUTCOME` also recognizes `DATA_PURGED` (6), meaning a
separately approved data purge was committed; pinned running revisions can
delay physical reclamation.

## Private application data (A5)

`osaiku_runtime::private_data` uses syscall 396 and `exchange` on the native
target (`exchange_with` for transports/tests). `READ` returns up to 4096 bytes
at an offset; `REPLACE` atomically replaces a small file with up to 4096 bytes,
including an empty file. It is not a streaming append interface. Names contain
1–63 ASCII letters, digits, dots, underscores or hyphens; `.` and `..`, paths,
drive names and nonzero padding are rejected. Initialize records with `EMPTY`.
The compatibility requirement is `protocol::PRIVATE_DATA` (7), version 1.0.
Request and reply records are exactly 4200 and 4128 bytes respectively.

The request names neither a publisher nor a realm, endpoint or launch Context.
The kernel resolves the actual calling process, its current policy authority
and its exact held package lease. Boot applications and processes without a
resident package lease cannot use this interface. Copying a Context cannot
transfer data access. The dispatcher runs on the storage pump with owned
request bytes, rechecks policy before access, and clears every payload byte on
failure. `BUSY` is retryable; client helpers validate sequence, lengths and
unused bytes before exposing results.

An old running revision retains its data after an update or uninstall. A
separate purge stops future installations from inheriting that realm, while
running consumers keep their pinned realm until retirement. A durable policy
revocation still denies further access immediately. This client does not
authorize installation, purge, selected-file access or publisher trust.
