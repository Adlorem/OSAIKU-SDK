# Native report host example

This SDK-only native application consumes `osaiku_runtime::report::ReportView`.
It displays one registered report as the complete client view or mounts it in an
empty region of a surrounding host view. The report uses ordinary native
bindings, layout, client-side rasterization and immutable publication. Its
sample counter is explicitly example data; it queries no system service.

The example exercises the production SDK API: open, prepare, encode, commit
after ordinary submission, discard after refusal, and close on session teardown.
Changing the embedding mode closes the old report incarnation before opening
another. Both modes use the same compiled report and contract. Region IDs must
not collide with report IDs. The source and host scopes remain process-owned;
the example is not an assistant service and does not impersonate assistantd.

Build from the SDK root with the pinned toolchain and its `rust-lld` linker:

```console
cargo build --offline --release -p osaiku-report-window --features native --target targets/x86_64-osaiku-user.json -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem -Z json-target-spec
```

Set `CARGO_TARGET_X86_64_OSAIKU_USER_LINKER` to the toolchain's `rust-lld` if it
is not already configured. The only dependencies are public SDK crates.
The experimental deployment convention matches `window-hello`: the runtime
reads its identity from the OS-admitted System Update image slot in a separate demo ESP. The example
does not register a new application or grant update privileges. Do not overwrite
a production update image to deploy it. A permanent report launcher and the
assistant's region/window integration are still OS work.

The report workspace is static process-image memory to avoid placing Documents
on a native thread stack. Session metadata remains small and goes through the
existing hosted runtime page lifecycle. Closing the report scrubs workspace
content; backing is retained for reuse by the process and reclaimed at process
exit. The process-image size must pass the OS's normal mapped-image budget.
