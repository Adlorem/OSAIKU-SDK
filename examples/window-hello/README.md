# Native window hello

A complete `no_std` counter application using only `osaiku-runtime` and
`osaiku-ui`. `build.rs` compiles `hello.ui.html` and `hello.ui.css`; Rust binds
literal text, handles stable button IDs and supplies explicit shutdown readiness.
Reset is disabled at zero; pointer and keyboard activation use the same controls.

From the SDK root, with the pinned toolchain and rust-src installed:

```text
cargo build --offline --locked --release -p osaiku-window-hello --features native --target targets/x86_64-osaiku-user.json -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec
```

See [native runtime](../../docs/native-runtime.md) for the public linker setup.
The ELF is `target/x86_64-osaiku-user/release/osaiku-window-hello` unless a target
directory override is active. It is an OSAIKU executable, not a host program.
No OS checkout is needed to compile it.

The experimental host currently admits fixed application images. This example
uses app ID 7 and can be deployed into the **System Update image slot in a
separate disposable demo ESP**, replacing `EFI/BOOT/SYSUPD.ELF` there. Launch it
with that slot's existing shelf icon. The admitted app identity remains slot 7; window chrome is supplied by the OS.
This demonstrates the public window runtime, not registration of a new package.
That demo image cannot be used to operate the normal System Update application.
Keep the production ESP unchanged. No kernel renderer branch or test mode is
needed; the example submits the same generic SDK content as System Manager.

The OS repository provides:

```text
python tools/ci/test-sdk-native.py --example window-hello --output target/sdk-window-build
python tools/qemu/test-sdk-window.py --esp target/esp --elf target/sdk-window-build/osaiku-window-hello.elf --output target/sdk-window-guest
```

The first command exports only SDK sources to a temporary directory, builds
offline and checks ELF bounds/W^X, preserving source and binary hashes. The
second requires Linux QEMU/OVMF and a production-built ESP. It makes its own
copy, installs the ELF only there and checks open, disabled state, real HID
pointer/keyboard activation, reset and close/reopen. Use a fresh output directory.
This is focused guest evidence, not physical or performance qualification.
