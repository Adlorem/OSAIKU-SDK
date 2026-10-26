# Independent USB Boot mouse driver

This is a runnable Driver Kit example, identity `example.driver-kit.usb-mouse`.
It receives copied three-byte Boot Protocol reports and emits relative movement
and three buttons. It has no private OS dependency, PCI access, DMA, UI or test
entry point. All authority comes from the host's attachment-scoped handshake.

From the SDK checkout, build outside both checkouts:

```sh
python tools/check-driver-example.py --output /tmp/driver-example
```

The output contains `usb-mouse`, `driver.toml` and `build.json`, including the
ELF digest and dependency graph. The build requires the SDK's pinned nightly,
rust-src and the public target. Choose your own package identity and restrict
the manifest's USB match to your intended test mouse before distribution.
The example's class match covers USB HID Boot mice; it does not identify a
particular physical device or grant permission to replace protected input.

Use [driver-pack and driver-key](../../docs/driver-packages-v1.md) to sign the
ELF and create a public publisher request. Keep the private seed outside the
checkout and boot medium. In OSAIKU, open the `.osadkey` in Device Manager,
review its fingerprint and Driver + Input roles, then inspect/install `.osadrv`.
Selection and installation do not prove activation: inspect the device's active
identity/version and input events. A protected early-boot driver can remain
active; use the authenticated early-set update/reboot path for that case.

Keep a verified recovery boot and an independent input device. Before disabling
the example, release held buttons; the host removes its source on disconnect,
failure or revocation. An invalid/short/oversized report is rejected, never
interpreted as motion. A lost process receipt ends the process rather than
retrying against a new attachment. For `NoDevice`, check VID/PID/interface and
match policy. For trust errors, inspect publisher roles/revocation. For a staged
version, inspect dependencies and restart requirements. Do not bypass storage
read-only/recovery refusal to install a package.

An independent build is a build result. Actual package installation, execution,
device handling, crash/rebind and physical recovery require separate evidence.
The host contract fixtures live in `driver-runtime/tests/hid.rs` and
`driver-runtime/tests/process.rs`, outside this production example.

License: MIT, as declared in Cargo.toml. This example uses the public OSAIKU
runtime and contains no Linux source. Runtime notices are in
`driver-runtime/THIRD_PARTY_NOTICES.md`.
