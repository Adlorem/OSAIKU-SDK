# OSAIKU-SDK

The [Driver Kit v1](docs/driver-kit-v1.md) provides standalone `no_std` protocol,
lifecycle, resource-lease and bounded-request contracts in `osaiku-driver-protocol`
and `osaiku-driver-runtime`. It has no private OS or UI dependency; hardware
authority and process transport remain responsibilities of the host platform.

Stage 30 development starts with experimental `osaiku-ui`: bounded HTML/CSS-like
compilation, versioned components, binding, layout, paint primitives and
semantics. See [UI format and current limits](docs/ui-v1.md). This is not yet a
released native application SDK. New sources are MIT licensed; the crate has
no third-party or private OS dependency.

The [report contract API](docs/report-contracts.md) adds versioned component,
data and action discovery, typed source registration and validation through the
same native compiler. The CLI generates the [catalog reference](docs/ui-catalog.md)
and references for host-registered schemas. The [report host](docs/report-host.md)
adds transactional updates and embedded regions, with a native
[report-window example](examples/report-window/README.md). The OS now has an
admitted report application. [Definition transfer](docs/report-transfer.md) provides
a native receiver and producer API. The OS connects its built-in catalog through
initd to report windows and an assistant presentation process with an owned region.
[Scoped read-only queries](docs/report-queries.md) now provide bounded metadata
scans, sorting/filtering and native background completion.
[Coherent window context](docs/window-context.md) supplies owner-authorized image
  and semantic snapshots from one committed publication.
  [Versioned report persistence](docs/report-persistence.md) saves definitions
  for fresh authorized execution and historical snapshots of materialized views.

Native views inherit shared SDK control styles based on System Manager.
Application CSS can override individual properties while keeping system defaults;
semantic palette tokens resolve against the host's selected palette. The current
OS shell uses Dark; it has no live theme-switching control. Contrast remains an
SDK host option. See [shared controls and palettes](docs/ui-v1.md#shared-native-controls-and-live-system-themes).

The shared [native frame and effects](docs/window-decoration.md) include the
HTML/CSS frame template, placement, configurable rounded outline and shadow,
plus optional backdrop glass. `WindowStyle` / `PreparedStyle` share validated
geometry and cached effect tables; the built-in native preset is `frame::STYLE`.
They operate on caller-owned RAM; linking the library grants no display access
or process isolation.

`osaiku_ui::notice` provides a shared HTML/CSS message recipe and a bounded
`text_model::Model` for its title, explanation, diagnostic detail and dismiss
label. Hosts compile `notice::HTML` with `notice::CSS`, substituting the palette
tokens `$SURFACE`, `$TEXT`, `$EDGE`, `$MUTED` and `$ACCENT`. The host handles
`notice::DISMISS` and owns visibility. OSAIKU uses this component for shell-owned
launch failures, including failures before an application can open a window.

Native hosts with small stacks should use `Scene::prepare_in` with a reusable
`layout::Workspace::EMPTY` beside their retained `Layout`. The hosted runtime
already owns both under its raster guard. This moves 3 KiB of measurement and
scroll arrays off the recursive layout stack; `Scene::prepare` remains available
for callers with sufficient stack space.

The experimental [native runtime foundation](docs/native-runtime.md) adds shared
process addresses, endpoint-aware IPC, bounded application message routing,
session-memory and teardown contracts,
window metadata/codecs, typed UI submission and event reception,
syscall traps, a public target/linker and an SDK-only native
ELF example. The OS consumes that same source. The [application package v1
tools and contracts](docs/application-packages.md) add signed packages,
publisher policy and signed repository indexes; runtime installation and
catalog integration are still separate OS work. The public single-window host
runtime and [window-hello](examples/window-hello/README.md) example cover
HTML/CSS compilation, input, submission and session teardown. Deployment still
requires an OS-admitted image; package registration is outside this subset.

OSAIKU is an experimental operating system. It is not a Linux distribution, a Windows modification, or a wrapper around another operating system. Its kernel, userspace services, application model, security boundaries, desktop environment, and storage stack are being developed from scratch. OSAIKU remains early-stage, non-production software intended for research, development, and controlled hardware testing.
The system already boots through UEFI on QEMU and physical hardware. It currently provides a custom kernel, process isolation, scheduling, typed system calls and IPC, keyboard and mouse support, a graphical desktop, movable multi-window composition, application lifecycle management, a Document application, File Manager, System Manager, and an initial persistent storage and native filesystem implementation. Work is continuing on full-disk installation, scalable storage, multicore execution, hardware drivers, performance, and recovery.
OSAIKU’s long-term goal is to provide a secure shared environment for humans and local AI agents. AI should understand the system through structured interfaces, explain its state, and assist with development without becoming the security authority. Kernel policies, explicit permissions, resource limits, and human approval remain responsible for privileged actions.
Most importantly, OSAIKU is intended to evolve around its user’s needs. With AI-assisted development, an ordinary user should be able to describe, design, review, test, and deploy new operating-system features without first becoming an experienced kernel engineer. Rather than being a fixed product, OSAIKU aims to become a transparent and adaptable system that users can gradually shape for their own hardware, workflows, and priorities.

## RELEASE PLAN

First experimental release is planned in December 2026.
