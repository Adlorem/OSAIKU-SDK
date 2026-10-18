use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rerun-if-changed=hello.ui.html");
    println!("cargo:rerun-if-changed=hello.ui.css");
    let view = osaiku_ui::compiler::compile(
        &std::fs::read_to_string("hello.ui.html").unwrap(),
        &std::fs::read_to_string("hello.ui.css").unwrap(),
    )
    .unwrap_or_else(|error| panic!("Window hello: {error}"));
    let mut bytes = [0; osaiku_ui::MAX_WIRE];
    let len = view.encode(&mut bytes).unwrap();
    std::fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("hello.ui.bin"),
        &bytes[..len],
    )
    .unwrap();
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("none") {
        let script = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
            .join("../../runtime/linker.ld");
        println!("cargo:rerun-if-changed={}", script.display());
        println!("cargo:rustc-link-arg=-T{}", script.display());
    }
}
