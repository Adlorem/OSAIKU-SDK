# Application packages v1

`osaiku-package` is the single `no_std`, allocation-free source for package,
compatibility, repository, publisher-policy and system-release validation. It
has no OS checkout dependency and no private-key loading code. `osaiku-runtime`
reexports compatibility types; the OS reexports exactly those same types.
The host Python serializer is checked against the shared Rust parser before
publishing output. It is not a second verifier.

These tools create and verify artifacts. They do not install or run them.
Package signatures do not grant capabilities or system roles. Themes uses the
public `appearance` and `picker` clients and reads the generic kernel-issued
launch context through `hosted::run`. Its application-owned image decoder and
view helper require no private OS ABI. `examples/window-hello/package.toml`
provides a second independent package, `dev.sdk-examples.counter`; it uses no
built-in application slot. Both are OSAIKU executables, not host programs.

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

## Application categories

Set `category = "system"` (for example) in an application's `[package]`
table. Accepted stable identifiers are `other`, `system`, `utilities`, `office`,
`graphics`, `multimedia`, `internet`, `games`, `development`, and `education`.
An unknown identifier is refused before publishing a package. Categories are
signed discovery metadata; they grant no permissions, roles or trust.

The shell supplies category labels independently of these stable identifiers;
the current Start menu uses Polish labels. Its all-applications tab is a filter,
not a manifest category. System-wide multilingual support is planned separately;
stable category identifiers must not become translated strings. Packages without
the field remain readable and use the `other` category.

The release manifests classify Themes and System Manager as System, Document
as Office, and the native/window SDK examples as Development. These rebuilt
packages target a fresh system installation; no on-disk catalog migration is
implied. The Start grid derives entries only from native installed packages.
It uses the package display name and icon; no separate shortcut file is needed.

A fresh system installation provisions Themes and System Manager into the native
application store using the authenticated release's application publisher and
the normal verified installation transaction. Document stays optional. Default
provisioning runs only during fresh installation, before boot publication;
normal boot/update never recreates a default package the user uninstalled.

Declare one `[[resources]]` entry with `kind = "icon"`, its logical `name`, and
the relative `path` of an RGBA resource as specified below. The desktop uses
existing named shell artwork for known icons and a bounded 8×8 thumbnail for
other package icons, with a fallback when the package has no visible icon.
Known names are `abi::shell_ui::APP_ICONS`; the release packages carry the
same artwork as their `icon.rgba` (`tools/icons/package-icons.py`), so a
package and the desktop show one picture.

The shell caches the complete installed discovery metadata within the native
store's existing 32-application bound. Catalog mutations refresh this snapshot
before completion even while Start is closed; unchanged digests reuse their
metadata. Opening, filtering and scrolling a ready snapshot require no storage
request. Serviced warms it after boot/remount, and explicit refresh retries a
failed fill. Cache metadata never grants launch authority: catalog generation,
policy and selected package identity are still checked when launching.

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
| 0 / 8 / 12 | `OSAMNF01`, u32 manifest version 1 or 2, u32 metadata size |
| 16 / 24 | u64 monotonic release / requested permission bits |
| 32 / 40 | u64 memory budget / I/O budget |
| 48 / 50 / 52 | u16 threads (1–16), instances (1–64), surfaces (0–16) |
| 54 / 56 / 58 | u16 entry (1 native, 2 window), preferred width/height |
| 60 / 62 | u16 compatibility length / component count |
| 64 / 192 | 128-byte ID / 128-byte display name |
| 320 / 384 | 64-byte version label / 64-byte SDK label |
| 448 / 450–511 | v2: u16 category / zero; v1: all bytes 448–511 zero |

Manifest v2 category numbers are 0 Other, 1 System, 2 Utilities, 3 Office,
4 Graphics, 5 Multimedia, 6 Internet, 7 Games, 8 Development, 9 Education.
All other numbers, unknown manifest versions and nonzero reserved bytes are
rejected. The packer emits v2 when the TOML explicitly declares a category;
omitting it retains canonical v1 encoding and the existing known-answer vectors.
The signed envelope and component format remain v1; a category change alters
signed metadata and therefore the complete package digest.

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

## Installed catalog observations (A5)

`osaiku_runtime::packages` provides read-only syscall 395. `STATUS` obtains the
current mount, catalog revision and policy epoch; `ITEM` reads one of at most
32 active entries against that exact snapshot. Restart enumeration on `STALE`.
`OUTCOME` looks up the durable operation ID for the calling process generation;
a reused PID does not inherit another caller's receipt. Requests are 80 bytes,
replies 288 bytes; `read_with` rejects malformed or uncorrelated replies.

The kernel lazily loads the existing authenticated store on its storage pump.
A missing or invalid policy/catalog is unavailable, never implicitly seeded
with a test key. Installation, trust decisions, launch, cancellation and private
data access are not operations in this observation protocol. The protected runtime UI and broker protocol below perform those operations.
Private data has a separate client described below. `OUTCOME` also recognizes `DATA_PURGED` (6), meaning a
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

## Runtime local installation and protected consent (A5)

On a mounted authenticated OSAIFS volume, use Package Manager → **Zainstaluj
z pliku** to select a publisher's raw 32-byte Ed25519 **public** key with the
`.osakey` extension. File Manager is independent of package installation. Compare
both halves of the SHA-256 fingerprint in the protected system panel before
choosing **Zaufaj temu kluczowi**. This authorizes the Application domain only.
It explicitly provisions the first policy or adds a key at the next policy
epoch; existing domain records and revocation tombstones remain. Opening a key
alone writes nothing. The package's signature does not contain a trusted key,
and the host tool's trust decision does not approve runtime installation.

Use the same Package Manager source browser to select `.osapp` from native
disk or a selected USB FAT directory. The kernel
revalidates the exact row/object/version or attachment/directory entry, streams
and verifies the complete package, then shows publisher, both digest halves,
release, requested rights, policy/catalog/operation generations, package size
and store quota. Confirm installation/update or explicitly choose rollback.
An incompatible package is refused. The transaction rereads and verifies the
source; replacement, disconnection, changed policy and stale catalog consent
cannot publish a different package. No scripts execute and installation never
automatically starts the program. **Uruchom** is a separate protected decision.

Package Manager opens with a paged list of native installations, retained data
and applications bundled in the signed system release. Each row offers
**Szczegóły**, **Uruchom** and **Zainstaluj**; unavailable actions are disabled.
Details retains the running-instance count, required-component status and
management actions, with **Wróć do listy** returning to the same page. Bundled
applications can be installed permanently on the native disk through the same
verified store transaction. A missing publisher trust decision is presented
separately before install consent; the OS signing key is never silently trusted. Uninstall preserves data; selecting retained data exposes a separate
purge confirmation. Refresh reconciles pending durable receipts and performs
bounded collection; live and required revisions remain protected. An uncertain
commit is reported as an error and is never blindly retried. Recovery from a
torn filesystem root still depends on A8 recovery work.

`osaiku_runtime::package_service` (syscall 397) is restricted to the actual
`serviced`/`initd` endpoints. Request/Reply sizes are 32/64 bytes. `POLL`,
`EXECUTE`, `STATUS` and `CANCEL` carry only a sequence and opaque kernel token,
never consent, a key, a path or permissions. Phases are 0 idle, 1 offered,
2 executing, 3 committed/completed, 4 cancelled, 5 failed. Replies correlate
sequence/token and bounded progress; `serviced` redeems storage jobs and only
`initd` redeems a launch. Both poll between normal service transactions.
One kernel-owned job retains approval and outcome across a service restart;
repeated EXECUTE cannot repeat an admitted or completed operation. On reboot,
the authenticated Store reconciles Pending receipts as Interrupted, without
replaying installation. Pins and private data belong to the kernel resident
owner until exact process retirement.

## Protected file picker (A5)

`osaiku_runtime::picker`, syscall 398, implements compatibility protocol
`SELECTED_FILES` (3), version 1.0. OPEN names the caller's exact live window,
incarnation and requested selected-file READ/WRITE rights. Use a new nonzero
sequence for each new chooser; repeating the same OPEN returns the same token.
STATUS polls that token. The user selects **Wybierz plik dla aplikacji** in File
Manager, reviews the target application's identity and rights, proceeds to the
file list, opens a file, then approves **Udostępnij ten plik**. An OPEN request,
manifest right or visible file row alone grants no access.

The token binds actual endpoint, launch context/policy, window incarnation and
exact native object/version or USB attachment/entry. READ returns up to 4096
bytes at an offset. REPLACE atomically replaces a native file with 0–4096 bytes
and advances only that scope's expected version. USB scopes are read-only;
requesting WRITE for USB is refused. REPLACE is a bounded small-file operation,
not append or chunked overwrite. CANCEL releases a pending request or scope.
Closing the window, process reuse, changed authority/policy and detached media
invalidate access. There are at most 64 scopes and one pending chooser.
Request/Reply sizes are 4168/4136 bytes; initialize with EMPTY. Failed replies
contain no file data. Ordinary apps never pass raw paths, object IDs or another
process's Context to this protocol.

CREATE (6) uses the same window binding and requires exactly WRITE. Its payload
is one UTF-8 filename of 1–64 bytes, without path separators, controls, `:`,
`.`/`..`, or a trailing space/dot. In Files, navigate to the destination folder,
choose **Select file for application**, and approve the protected create dialog.
The scope binds that folder's mount/object/version and the exact filename.
REPLACE publishes the complete new file atomically, only if the name is absent;
it never overwrites a collision or first exposes an empty placeholder. Success
turns this scope into a version-bound file WRITE scope. A refused, stale or
uncertain operation requires an explicit user retry; preserve the draft.

`picker::OpenScope` is a 40-byte authenticated host message delivering a READ
scope for a file opened in Files. Accept it only through `hosted::host_message`,
for the exact window/incarnation and a fresh sequence, while the document can
safely accept replacement. Return its correlated 24-byte acknowledgement. The
token still has to pass STATUS/READ; receiving bytes does not confer authority.
Release it with CANCEL after the read. The migrated Document uses this path,
public picker transactions, and app-owned text/save state. Its current text
limit is 256 UTF-8 bytes; overlong input is refused without truncation.

## Protected browser client and metadata

File Manager owns its view templates, labels, menu interpretation, double-click
state and navigation. `file_manager`, `file_browser`, `file_export`,
`directory_summary`, `threads` and the native `desktop` client live in the SDK.
These definitions do not grant the protected File Manager profile to an ordinary
signed package. The OS retains storage queues, object/version validation, copy
transactions, durability, picker consent and protected recovery/update profiles.

Each browser publication includes a `file_broker::Map` of element IDs to typed
operations and object versions. This map declares possible operations; it is not
authority. The OS validates it against the submitted inventory, then issues a
two-second, single-use `Receipt` only for actual input on that presented window.
TAKE/EXECUTE bind caller, window, incarnation, presentation revision/epoch,
token, operation and object/version. Stale or replayed commands fail. Pointer
gesture discontinuity is reported as an epoch; the app decides click behavior.
No menu label or private OS template is part of this authorization protocol.
EXECUTE returns LOCAL (0) for an app-owned state transition and HANDLED (1)
when server work has completed or been queued. Do not repeat HANDLED operations
locally; in particular, USB directory IDs must not become native-disk requests.
The OS verifier's `--example file-manager --files-menu-probe` rebuilds a changed
menu outside the OS checkout using the SDK alone.

`window_status` and `status_client` provide the bounded metadata exchange used
by the Assistant: exact presentation identity, text length and save state only.
Register the bounded handler, publish after submission, and invalidate it on
model changes until the next submission. Never send document text or a content
hash in this response. Physical input and the OS grant select the target;
registration alone does not authorize queries. Applications using a custom
desktop loop must explicitly answer shutdown preparation after checking their
pending work; generic IPC/yield wrappers never silently acknowledge readiness.

Repository browsing and HTTPS download remain explicitly dependent on Stage 33
network transport, trusted time and certificate/DNS destination policy (A5-N).
They are not provided by a parser or this local disk/USB workflow. Physical
startup/latency/RAM qualification remains a separate acceptance requirement.

## Catalog launcher and independent applications (A6)

Open **Files → Applications**, select the catalog item with Previous/Next and
choose **Launch**. The protected snapshot binds the exact catalog revision,
policy epoch, mount, item digest, publisher, permissions and view generation.
Stale clicks are refused. Installing and launching are separate actions.
`initd` alone redeems the launch token, then authorizes and creates the exact
endpoint's window through the compositor. Opening/presentation failure is
reported in the panel; unrelated application windows are preserved. The broker
retains a successful process result until initd acknowledges `OPENED` or
`OPEN_FAILED`. A restarted initd preserves an already created window.

The launcher shows the signed display name and a monochrome 8×8 thumbnail,
scaled to 32×32. RGBA components up to 256×256 are sampled in at most eight
bounded row reads; eight digest-keyed thumbnails are cached. Missing icons use
a fixed fallback. Catalog mutation notifications coalesce refreshes of a visible
panel; a hidden panel does not poll the catalog. Existing retained rendering
handles window damage; icon decode never runs during painting.

Themes' **Choose image** asks for a read-only selected-file scope. In Files,
choose **Select file for application**, browse the native disk or USB, open the
image and approve the protected dialog. Themes reads only that opaque scope in
4 KiB chunks. Its two wire payload buffers live in a retained workspace through
`picker::exchange_into`, away from the decoder’s 16 KiB thread stack. Cancel/reselection/window close release scopes. Changing desktop
appearance still requires the independently approved appearance permission.
The wallpaper is system appearance state; it is not a private application file.

The OS build verifier exports Themes, its own `image` library and the public
SDK into a temporary workspace. `tools/ci/test-sdk-native.py --example themes`
and `--example window-hello` preserve source/ELF hashes and validate mappings.
`tools/ci/test-installed-sdk-apps.py --output target/application-installation/a6-installed`
builds both, creates **test-only** signed fixtures and exercises actual OSAIFS
installation, image authentication and the shared loader's page recipes. The
fixture uses a public test seed; it is not a production publisher or a physical
execution qualification. Use your own publisher key for actual installation.


## System monitor client

The migrated OSAIKU System Manager builds with the public SDK only and requests
`SYSTEM_METRICS_READ` (8). Use `metrics::read(None)` for the current bounded
sample and `metrics::read(Some((page, sample)))` for a page of that exact sample.
`metrics::select(0, 0)` selects the overview; `(0, 1)` selects processors.
`select(1, 1)` and `select(1, u64::MAX)` move the provider page forward/back.
Other selections fail before crossing the syscall boundary. These operations
never grant process control or make all of the application's UI text public.

The application owns measurement history, row identities, scrolling and labels;
`hosted::run` owns the common session/presentation transport. Its manifest lives
at `apps/system_manager/package.toml` in OSAIKU. The OS verification command is
`tools/ci/test-sdk-native.py --example system-manager --output OUTPUT`;
`test-installed-sdk-apps.py --a7` also admits its real ELF through the signed
package store and production loader's page validation. These host checks do
not qualify CPU execution, fresh installation, or physical hardware acceptance.


## Revision-7 system distribution

The OS release builder emits `OSAIKERN.ELF`, `OSAIBASE.PKG`, `RELEASE.SIG`
and `APPS.PKG` beside `BOOTX64.EFI`. `OSASYS01` authenticates the exact kernel
and base pair, handoff revision 7 and service ABI 1. `OSABASE1` contains a fixed
recovery profile and role 32 catalog metadata; only the kernel interprets roles.
`OSACAT01` carries bounded ordinary package metadata and hashes. The executable
bodies remain in `APPS.PKG` on USB or the selected installed bank and are verified
again when launched. A package name cannot grant a protected service role.

The installer and system updater publish the complete catalog with the bank.
Both retained banks keep their own application versions. These distribution
versions are pinned to the system release; the ordinary mutable application
catalog remains independent. Launch contexts use `BANK_PACKAGE` for a pinned
release version and `PACKAGE` for a mutable installation, both ordinary identities.
User package policy changes revoke mutable installations, not the authenticated
recovery distribution. Pinned versions appear as installation sources in the independent Package
Manager. Start lists only native installed applications; ordinary installations
retain the update, rollback and uninstall flow. Live USB does not require NVMe or a mutable
application store to reach the installer and recovery tools.

Validate an assembled release without firmware using:

```text
cargo run --manifest-path sdk/Cargo.toml -p osaiku-package --example system-release-check -- target/esp/EFI/BOOT target/system-release-public.bin
```

The signing seed stays outside the checkout. This is software release
verification against a compiled public key, not a claim of hardware Secure Boot.
Old bank manifests and revision-6 handoffs are unsupported; use a fresh install.


The shell carries the verified manifest display name and icon resource name with
the dynamic application identity into window decorations. A matching original
shell SVG image set retains its theme selection; these presentation names never
grant a service role or replace the issued application ID. Start catalog rows are
created by installation and removed by uninstall, independently of the recovery
release's package catalog.
