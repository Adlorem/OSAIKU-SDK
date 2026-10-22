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

Package the ELF using [package.toml](package.toml) and the public
[package tools](../../docs/application-packages.md). Approve the publisher key,
install the `.osapp` from disk/USB and choose **Files → Applications → Launch**.
The kernel issues a dynamic identity for `dev.sdk-examples.counter`; initd opens
its ordinary SDK window without replacing any boot image or restarting the OS.

The OS repository can verify the independent build with:

```text
python tools/ci/test-sdk-native.py --example window-hello --output target/sdk-window-build
```

This exports SDK sources to a temporary workspace, builds offline and checks
ELF bounds/W^X, preserving source and binary hashes. Build and host-contract
success do not prove physical execution or interactive UI behavior.
